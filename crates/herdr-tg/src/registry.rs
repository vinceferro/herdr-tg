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
                "the new secret was written into {}, but the list of projects could not be saved — \
                 so that project's own copy no longer matches and its bridge will be turned away \
                 until you run this again:\n\
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

    /// Switch a project off, or back on. Terminal-only, like everything else that changes who may
    /// connect: no message and no frame reaches this.
    ///
    /// The project is found by its repo path, canonicalised when the folder still exists and taken
    /// as written when it does not — a project whose tree has been deleted is exactly the one worth
    /// switching off, and refusing because the folder is gone would leave it the only project on
    /// the box that cannot be.
    ///
    /// Writing the flag is only half of what "off" means. The hub reads `enabled` at `hello`, so
    /// this alone turns away the NEXT connection and does nothing to one already on the socket. The
    /// other half — dropping a live connection — is the hub's, which watches this file for exactly
    /// that. See `Hub::drop_connections_of_switched_off_projects`.
    pub fn set_enabled(&mut self, repo: &Path, enabled: bool) -> Result<Project, EnrolError> {
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let wanted = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
        let Some(p) = self.projects.values_mut().find(|p| p.repo == wanted) else {
            return Err(EnrolError::NotEnrolled {
                repo: repo.to_path_buf(),
            });
        };
        p.enabled = enabled;
        let project = p.clone();
        self.save()?;
        Ok(project)
    }

    /// Let a person speak in this project's conversations, or stop them. Terminal-only, like the
    /// switch: no message, no tap and no frame reaches this, and `nothing_inbound_can_add_a_person`
    /// fails the build if the bot or the hub ever names it.
    ///
    /// The same read-modify-write as `set_enabled`, found by the same path rule, and live for the
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
        let wanted = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
        let Some(p) = self.projects.values_mut().find(|p| p.repo == wanted) else {
            return Err(EnrolError::NotEnrolled {
                repo: repo.to_path_buf(),
            });
        };
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

    /// Enrol a repo, or rotate an already-enrolled one's secret.
    ///
    /// Terminal-only. Returns the secret exactly once, because it is never stored anywhere this
    /// process can read it back.
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
        let repo = repo.canonicalize().map_err(|_| EnrolError::NoSuchRepo {
            repo: repo.to_path_buf(),
        })?;
        if !repo.is_dir() {
            return Err(EnrolError::NoSuchRepo { repo });
        }

        // Both paths are canonical, so this is a real containment test and not a string prefix that
        // would call `/srv/app2` a child of `/srv/app`. Re-enrolling the same folder is a rotation
        // and stays allowed; only a folder BELOW an enrolled one is refused.
        if let Some(parent) = self
            .projects
            .values()
            .find(|p| repo != p.repo && repo.starts_with(&p.repo))
        {
            return Err(EnrolError::InsideAnotherProject {
                repo,
                parent: parent.repo.clone(),
                title: parent.title.clone(),
            });
        }

        // Minted from the canonical path, never from a counter. A recycled counter silently
        // inheriting a dead agent's topic is a defect this repo has already shipped once.
        let id = ProjectId::new(format!(
            "p-{}",
            &sha256_hex(repo.as_os_str().as_encoded_bytes())[..12]
        ));

        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|_| EnrolError::NoRandomness)?;
        let secret = hex(&secret);

        let title = self.unique_title(&repo, &id);
        let icon_color = (u8::from_str_radix(&sha256_hex(id.as_str().as_bytes())[..2], 16)
            .unwrap_or(0))
            % ICON_COLOURS;

        // A re-enrolment keeps the topic. The whole point of a stable id is that history survives.
        let topic_id = self.projects.get(&id).and_then(|p| p.topic_id);

        // A re-enrolment keeps its lanes' topics for the same reason it keeps its own: rotating a
        // secret must not scatter a day's worktrees into a second set of topics beside the first.
        let lane_topics = self
            .projects
            .get(&id)
            .map(|p| p.lane_topics.clone())
            .unwrap_or_default();

        // A re-enrolment keeps the switch where the operator left it. This was `true` outright, so
        // rotating a leaked secret — the documented reason to re-run `enroll` — silently switched a
        // project he had turned off back on, and nothing in the output said so.
        let enabled = self.projects.get(&id).is_none_or(|p| p.enabled);

        // A re-enrolment keeps the project's people, for the reason it keeps the switch: each of
        // them was let in by a decision at a keyboard, and rotating a secret is not a decision
        // about who may speak. Dropped here, a rotation would silently shut a room's own people
        // out — and nothing in the output would say so.
        let allowed_users = self
            .projects
            .get(&id)
            .map(|p| p.allowed_users.clone())
            .unwrap_or_default();

        let project = Project {
            id: id.clone(),
            title,
            repo: repo.clone(),
            token_sha256: sha256_hex(secret.as_bytes()),
            enabled,
            topic_id,
            icon_color,
            lane_topics,
            allowed_users,
        };
        // The SECRET GOES DOWN FIRST, and the list of projects second. The other order took a
        // project off the air whenever the second step failed: the registry already held the hash
        // of a secret that had never been written, the repo still held the old one, and the old one
        // no longer resolved — so the bridge was refused on every reconnect and nothing on the box
        // said why. What the operator had been told was "could not write the project's token file",
        // which reads as a command that did nothing. A full disk, a read-only mount, or a
        // `.kickoff` left root-owned by one sudo run is all it takes.
        //
        // This way round the same failure changes nothing at all: no hash is saved, and the secret
        // already in the repo goes on working.
        write_token_file(&repo, &secret)?;
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
    fn hold(&self) -> Result<fs::File, EnrolError> {
        let path = self.path.with_extension("lock");
        let io = |source| EnrolError::Io {
            what: "the registry lock".to_owned(),
            source,
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io)?;
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
            fs::create_dir_all(dir).map_err(io("the state directory"))?;
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

fn sha256_hex(bytes: &[u8]) -> String {
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
        let off = r.set_enabled(&repo, false).expect("switches off");
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
        let on = r.set_enabled(&repo, true).expect("switches on");
        assert!(on.enabled);
    }

    #[test]
    fn switching_a_project_nobody_enrolled_is_refused_in_plain_words_rather_than_enrolling_it() {
        // Switching on is not a way in. A command that quietly enrolled when asked to flip a switch
        // would be a second door beside the one that has the guard on it.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let said = r
            .set_enabled(&repo(&d, "never-enrolled"), true)
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
            .set_enabled(&p.repo, false)
            .expect("a deleted folder can still be switched off");
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
}

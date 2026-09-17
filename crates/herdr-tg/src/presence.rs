//! Who has a bridge on the socket right now, written by the running hub for a process that is not
//! the hub to read.
//!
//! # Why a file, and not a question over the socket
//!
//! The live claims map exists in exactly one process. `herdr-tg projects --json` runs in another,
//! and the room-map handshake it serves needs `connected` to come from that map and from nothing
//! else — a topic binding is permanent from the first connection on and says nothing about now.
//! Three ways for a second process to learn it were weighed:
//!
//! * **A query over the hub's socket.** Every connection there presents a secret, and a secret
//!   proves one project; a query that needed none would be a new frame kind, which is a change to
//!   the wire contract this slice was told not to make.
//! * **The lock file.** `hub.lock` is the kernel's own answer to "is a hub running", but probing it
//!   means taking the lock for an instant, and a hub starting in that instant is refused with
//!   "another copy is already running" and restarted five seconds later by systemd. A read-only
//!   command must not be able to do that to the thing it is reading about.
//! * **A snapshot the hub writes**, whenever the map changes. Cheap, and the hub already keeps
//!   three state files this way. Its one hazard is staleness: a hub killed with `SIGKILL` leaves a
//!   file naming claims that died with it. So the snapshot carries the hub's pid, and a reader
//!   believes it only when that pid is alive, is running under one of the hub's own command names,
//!   and is the pid the lock's holder wrote — which the same process wrote at the same moment it
//!   took the lock.
//!
//! When none of that holds the answer is **unknown**, never `false`: a reader that cannot prove
//! nothing is connected must not say so. A hub that is not running has nothing connected to it,
//! but the moment between one hub dying and the next one's first write is a moment in which a
//! confident `false` would be a guess, and the whole point of this file is not to guess.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use hub_proto::{LaneId, ProjectId};
use serde::{Deserialize, Serialize};

use crate::hub::Addr;

/// The file's name inside the state directory.
pub const FILE: &str = "hub.connected.json";

/// One live connection, as the hub wrote it down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connected {
    pub project: ProjectId,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lane: Option<LaneId>,
}

impl Connected {
    pub fn addr(&self) -> Addr {
        Addr {
            project: self.project.clone(),
            lane: self.lane.clone(),
        }
    }
}

/// What the hub wrote, and which hub.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The process that wrote this. The reader's whole test of whether to believe it.
    pub hub_pid: u32,
    /// Seconds since the epoch, for a person reading the file; nothing decides on it.
    pub at: u64,
    pub connected: Vec<Connected>,
}

/// The hub's side: the file, and how to write it.
#[derive(Clone, Debug)]
pub struct Presence {
    path: PathBuf,
}

impl Presence {
    /// The hub makes one beside its audit log; the inventory reads `<state dir>/hub.connected.json`
    /// through [`vouched_for`]. There is deliberately no default path here — the file is the hub's
    /// to place, and a second derivation would be a second place for the two to disagree.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write down who is connected, whole, under this process's pid.
    ///
    /// Temp-and-rename at 0600, the discipline every other state file here keeps: a reader must
    /// never see half a list, and this one names which projects are live on the box.
    ///
    /// A rewrite that FAILS takes the previous snapshot with it. Temp-and-rename leaves the old
    /// file in place on a full disk or a filesystem gone read-only, and that file carries this
    /// hub's own pid — so every check the reader makes holds and it goes on naming bridges that
    /// have since left, for as long as this hub lives. Freshness is nothing a reader can test; the
    /// writer has to make a failed write readable as "unknown", and unlinking needs no free space.
    pub fn write<'a>(&self, connected: impl IntoIterator<Item = &'a Addr>) -> std::io::Result<()> {
        let written = self.write_whole(connected);
        if written.is_err() {
            let _ = fs::remove_file(&self.path);
        }
        written
    }

    fn write_whole<'a>(
        &self,
        connected: impl IntoIterator<Item = &'a Addr>,
    ) -> std::io::Result<()> {
        let snapshot = Snapshot {
            hub_pid: std::process::id(),
            at: crate::hub::now_secs(),
            connected: connected
                .into_iter()
                .map(|a| Connected {
                    project: a.project.clone(),
                    lane: a.lane.clone(),
                })
                .collect(),
        };
        if let Some(dir) = self.path.parent() {
            crate::conversations::private_state_dir(dir)?;
        }
        let tmp = self
            .path
            .with_extension(format!("json.tmp.{}", std::process::id()));
        let body = serde_json::to_vec_pretty(&snapshot)?;
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

    /// Read the file as written, believing nothing about it yet. See [`vouched_for`].
    pub fn read(&self) -> std::io::Result<Snapshot> {
        let raw = fs::read_to_string(&self.path)?;
        serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// The snapshot in `state_dir`, if — and only if — a running hub stands behind it.
///
/// Three facts, all required, none a guess: the lock file names a holder; that holder is alive and
/// is a hub of ours (see [`HUB_COMMAND_NAMES`]); and the snapshot was written by that same pid.
/// Anything less is `None`, which the caller must render as UNKNOWN rather than as nothing
/// connected.
///
/// The lock file is read, never locked. Taking it, even non-blocking and for a microsecond, can
/// refuse a hub that is starting in that microsecond.
pub fn vouched_for(state_dir: &Path) -> Option<Snapshot> {
    let holder: u32 = fs::read_to_string(state_dir.join("hub.lock"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if !crate::transport::fence_is_alive(holder) || !looks_like_a_hub(holder) {
        return None;
    }
    let snapshot = Presence::new(state_dir.join(FILE)).read().ok()?;
    (snapshot.hub_pid == holder).then_some(snapshot)
}

/// Every executable name a hub of ours is allowed to be running under.
///
/// Three spellings because the product was renamed and the old name still ships:
///
/// * `kickoff-channel` — the canonical installed binary, and the name a fresh adopter invokes.
/// * `kickoff_channel` — the same crate built as a test binary; Cargo spells the target with an
///   underscore (`deps/kickoff_channel-<hash>`).
/// * `herdr-tg` and `herdr_tg` — the former name, kept as an alias for installs that predate the
///   rename. The running hub on a box that has not been reinstalled is still this one, so dropping
///   it here would make a live hub stop vouching for its own snapshot.
///
/// Both spellings of each name are listed because Cargo names a test target with an underscore
/// (`deps/kickoff_channel-<hash>`) while the shipped command keeps its hyphen. The predicate this
/// replaced matched the single prefix `herdr`, which covered both by accident; naming them is what
/// makes the rename visible here at all.
///
/// **Matched by prefix, and the prefixes are exact on purpose.** A looser `kickoff` would vouch
/// for any future sibling command that happens to share the stem — `kickoff-hub-attach` is
/// already on this box — and the whole point of the check is to be narrower than "some process
/// with this pid exists".
const HUB_COMMAND_NAMES: [&str; 4] = ["kickoff-channel", "kickoff_channel", "herdr-tg", "herdr_tg"];

/// Is this pid one of ours? `/proc/<pid>/comm` is the executable's name, and a hub runs under one
/// of [`HUB_COMMAND_NAMES`] whether it is the installed binary or a test build. A pid recycled by
/// some other program after a hub was killed fails this, which is the one case a live-pid check
/// alone would get wrong.
///
/// **`comm` holds fifteen characters.** The kernel truncates to `TASK_COMM_LEN - 1`, and
/// `kickoff-channel` is exactly fifteen — it survives whole, with nothing to spare. A future name
/// one character longer would arrive here already cut, and this check would silently stop
/// recognising the hub rather than fail loudly. `the_hub_is_recognised_under_every_name_it_ships_under`
/// pins the current names against that.
fn looks_like_a_hub(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|comm| comm_is_a_hub(comm.trim()))
        .unwrap_or(false)
}

/// The name rule on its own, so it can be tested without a process to point it at.
fn comm_is_a_hub(comm: &str) -> bool {
    HUB_COMMAND_NAMES.iter().any(|name| comm.starts_with(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(p: &str, l: &str) -> Addr {
        Addr::lane_of(ProjectId::new(p), LaneId::new(l))
    }

    /// Every name the hub can legitimately be running under, and the near misses it must refuse.
    ///
    /// This exists because the rename to `kickoff-channel` silently broke the check it pins: the
    /// predicate tested one hard-coded prefix, so the renamed binary — and every test build of it
    /// — stopped being recognised as a hub, and `vouched_for` began answering UNKNOWN about a
    /// perfectly live process. A name rule that is invisible when it is wrong needs a test that
    /// names each accepted spelling out loud.
    #[test]
    fn the_hub_is_recognised_under_every_name_it_ships_under() {
        // The installed binary a fresh adopter runs. Exactly 15 characters, which is all
        // /proc/<pid>/comm holds, so it arrives whole and with nothing to spare.
        assert!(comm_is_a_hub("kickoff-channel"));
        assert_eq!("kickoff-channel".len(), 15);

        // The same crate built as a test binary: Cargo spells the target with an underscore and
        // appends a hash, which comm then truncates back to exactly the stem.
        assert!(comm_is_a_hub("kickoff_channel"));

        // The former name, still installed on every box that predates the rename — including the
        // hub running right now. Dropping this arm would make a live hub disown its own snapshot.
        assert!(comm_is_a_hub("herdr-tg"));
        assert!(comm_is_a_hub("herdr_tg-b4a92115d4e3f"));

        // A pid recycled by something else is the case the liveness check alone gets wrong, and
        // the whole reason this predicate exists.
        assert!(!comm_is_a_hub("systemd"));
        assert!(!comm_is_a_hub("bash"));

        // Narrower than the stem on purpose: a sibling command that merely shares it is not this
        // hub. `kickoff` and `kickoff-hub-attach` are both already on this machine.
        assert!(!comm_is_a_hub("kickoff"));
        assert!(!comm_is_a_hub("kickoff-hub-att"));
    }

    #[test]
    fn a_snapshot_survives_the_file_and_names_its_writer() {
        let d = tempfile::tempdir().expect("tmp");
        let presence = Presence::new(d.path().join(FILE));
        let own = Addr::project_itself(ProjectId::new("p-one"));
        presence
            .write([&own, &lane("p-one", "lane-0905-a")])
            .expect("writes");
        let back = presence.read().expect("reads");
        assert_eq!(back.hub_pid, std::process::id());
        assert_eq!(back.connected.len(), 2);
        assert_eq!(back.connected[0].addr(), own);
        assert_eq!(back.connected[1].addr(), lane("p-one", "lane-0905-a"));
    }

    #[test]
    fn a_snapshot_is_believed_only_when_the_lock_holder_is_alive_and_wrote_it() {
        // Every way the file can be stale, in turn. None of them may read as "nothing connected".
        let d = tempfile::tempdir().expect("tmp");
        let presence = Presence::new(d.path().join(FILE));
        presence
            .write([&Addr::project_itself(ProjectId::new("p-one"))])
            .expect("writes");

        // No lock file: no hub has ever run here.
        assert!(vouched_for(d.path()).is_none(), "believed with no hub");

        // The lock names this very process, which is alive and is a herdr build.
        fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");
        assert!(
            vouched_for(d.path()).is_some(),
            "not believed with a live hub standing behind it"
        );

        // The lock names a process that is gone: a hub killed and not restarted.
        let mut child = std::process::Command::new("/bin/true")
            .spawn()
            .expect("spawn");
        let dead = child.id();
        child.wait().expect("reap");
        fs::write(d.path().join("hub.lock"), dead.to_string()).expect("lock");
        assert!(vouched_for(d.path()).is_none(), "believed a dead hub");

        // The lock names a live process that is not a hub: a recycled pid, or a hand-written file.
        // pid 1 is always alive and is never a hub of ours.
        fs::write(d.path().join("hub.lock"), "1").expect("lock");
        assert!(
            vouched_for(d.path()).is_none(),
            "believed a process that is not a hub"
        );

        // A live hub holds the lock, but the snapshot was written by a previous one: the window
        // between a restart and its first write.
        fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");
        let mut stale = presence.read().expect("reads");
        stale.hub_pid = dead;
        fs::write(
            presence.path(),
            serde_json::to_string(&stale).expect("json"),
        )
        .expect("write");
        assert!(
            vouched_for(d.path()).is_none(),
            "believed a snapshot the running hub did not write"
        );
    }

    #[test]
    fn a_snapshot_the_hub_could_not_rewrite_is_removed_rather_than_left_stale() {
        // Temp-and-rename leaves the previous file in place when the write fails — a full home
        // partition, a read-only filesystem after an error — and that file carries THIS hub's pid,
        // so the reader's every check holds and it goes on believing a list of bridges that have
        // since left. Freshness is not something the reader can test; the writer has to make a
        // failed rewrite readable as "unknown", and unlinking needs no free space.
        let d = tempfile::tempdir().expect("tmp");
        let presence = Presence::new(d.path().join(FILE));
        presence
            .write([&Addr::project_itself(ProjectId::new("p-one"))])
            .expect("writes");
        fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");
        assert!(vouched_for(d.path()).is_some(), "not believed while fresh");

        // The rewrite cannot happen: something sits where the temp file goes.
        let tmp = presence
            .path()
            .with_extension(format!("json.tmp.{}", std::process::id()));
        fs::create_dir(&tmp).expect("a directory in the way");
        presence
            .write(std::iter::empty())
            .expect_err("a write that could not happen was reported as done");
        assert!(
            !presence.path().exists(),
            "the previous snapshot was left in place after a rewrite failed"
        );
        assert!(
            vouched_for(d.path()).is_none(),
            "a snapshot the hub could not rewrite is still believed"
        );
    }

    #[test]
    fn the_snapshot_is_not_readable_by_anyone_else() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().expect("tmp");
        let presence = Presence::new(d.path().join(FILE));
        presence.write(std::iter::empty()).expect("writes");
        let mode = fs::metadata(presence.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "mode {:o}", mode & 0o777);
    }
}

//! One hub per state directory, enforced before anything else happens.
//!
//! # Why this runs before the bot exists
//!
//! Telegram's long-poll is a single slot. Two processes polling one bot token do not share it;
//! they take turns losing to each other, and each one sees HTTP 409. The visible result is a bot
//! that answers sometimes, a worker that looks healthy from the process table, and an operator
//! whose messages vanish — which this system has already paid for once, with an infinite backoff
//! and a 198-line `/proc` walk written to notice it after the fact.
//!
//! So the lock is taken **before the `Bot` is constructed**, and the failure is boring on purpose:
//! the second copy exits non-zero, immediately, naming the process that already holds the line.
//! `a_second_hub_never_reaches_the_token` pins the ordering by running the binary with no token at
//! all — if the lock were taken second, the error would be about the missing token instead.
//!
//! # Why an advisory lock and not a pidfile
//!
//! `flock` is released by the kernel when the holder dies, however it dies. A pidfile created with
//! `O_EXCL` is not: a SIGKILL leaves a file that only a human at a keyboard can clear, and the
//! operator this is built for is holding a phone. The pid written inside the file is for the error
//! message only — it is never what grants or denies the lock.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

/// The name of the lock inside the state directory.
const LOCK_FILE: &str = "hub.lock";

/// `$XDG_STATE_HOME/herdr-tg`, else `~/.local/state/herdr-tg`.
///
/// One derivation, used by the lock and by the heartbeat, so the two cannot come to disagree about
/// which directory this hub lives in. `audit.rs` and `routing.rs` each still carry their own copy
/// of this; they are older, they are load-bearing, and folding them in is a change to files this
/// slice has no other reason to touch.
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("herdr-tg")
}

/// Why the hub may not start.
#[derive(Debug)]
pub enum LockError {
    /// Another copy holds it. `pid` is absent when the holder has not written it yet — a real
    /// window, small, and reported honestly rather than filled in with a guess.
    Held { pid: Option<u32> },
    /// The lock could not be attempted at all.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Operator-facing. No "flock", no "EWOULDBLOCK", no path first.
            Self::Held { pid: Some(pid), .. } => write!(
                f,
                "another copy of this bridge is already running (process {pid}), and only one can \
                 hold the Telegram line. Stop that one first:  systemctl --user stop herdr-tg"
            ),
            Self::Held { pid: None, .. } => write!(
                f,
                "another copy of this bridge is already running, and only one can hold the \
                 Telegram line. It started moments ago and has not named itself yet; try again in \
                 a second, or stop it:  systemctl --user stop herdr-tg"
            ),
            Self::Io { path, source } => {
                write!(
                    f,
                    "could not use the state directory at {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Held { .. } => None,
        }
    }
}

/// Held for as long as this hub runs. Dropping it, or dying, releases the lock.
#[derive(Debug)]
pub struct HubLock {
    /// Kept alive solely to hold the lock: the kernel releases it when this descriptor closes.
    /// Never read again, which is exactly why it is easy to "tidy away" — and deleting it would
    /// release the lock at the end of `acquire`, leaving every hub convinced it was the only one.
    #[allow(dead_code)]
    file: File,
    path: PathBuf,
}

impl HubLock {
    /// Take the lock, or say who has it.
    ///
    /// Creates the state directory if it is missing. The lock file is mode 0600 — it holds no
    /// secret, but it lives beside the audit log and the routing state, and one permissive file in
    /// that directory is one more thing to reason about than none.
    pub fn acquire(state_dir: impl AsRef<Path>) -> Result<Self, LockError> {
        let state_dir = state_dir.as_ref();
        let path = state_dir.join(LOCK_FILE);
        let io_err = |source| LockError::Io {
            path: path.clone(),
            source,
        };

        // 0700 from the first touch, not from history: this is the first thing `serve` makes on a
        // fresh box, and every credential the channel keeps lives under it.
        crate::conversations::private_state_dir(state_dir).map_err(io_err)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(io_err)?;

        // Non-blocking on purpose. Waiting would turn "another copy is running" into a hang, and a
        // hang is the one failure a phone-only operator cannot tell apart from ordinary slowness.
        if let Err(errno) = flock(&file, FlockOperation::NonBlockingLockExclusive) {
            if errno == rustix::io::Errno::WOULDBLOCK {
                return Err(LockError::Held {
                    pid: read_pid(&mut file),
                });
            }
            return Err(LockError::Io {
                path,
                source: std::io::Error::from(errno),
            });
        }

        // Only now that the lock is ours may the file be rewritten. Truncating first would blank
        // the holder's pid for a contender reading it, turning a useful message into the vaguer one.
        file.set_len(0).map_err(io_err)?;
        file.seek(SeekFrom::Start(0)).map_err(io_err)?;
        write!(file, "{}", std::process::id()).map_err(io_err)?;
        file.flush().map_err(io_err)?;

        Ok(Self { file, path })
    }

    /// The lock file, for a diagnostic that wants to name it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Best effort. A holder that has locked but not yet written its pid yields `None`, and `None` is
/// reported as "it has not named itself yet" rather than dressed up as a fact.
fn read_pid(file: &mut File) -> Option<u32> {
    let mut s = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_to_string(&mut s).ok()?;
    s.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    #[test]
    fn the_first_hub_takes_the_lock_and_writes_its_own_pid() {
        let d = tmp();
        let lock = HubLock::acquire(d.path()).expect("the first hub starts");
        let written = std::fs::read_to_string(lock.path()).expect("readable");
        assert_eq!(written.trim(), std::process::id().to_string());
    }

    #[test]
    fn a_second_hub_is_refused_and_told_which_process_has_the_line() {
        let d = tmp();
        let _first = HubLock::acquire(d.path()).expect("the first hub starts");
        match HubLock::acquire(d.path()) {
            Err(LockError::Held { pid, .. }) => {
                assert_eq!(pid, Some(std::process::id()));
                let said = LockError::Held { pid }.to_string();
                assert!(said.contains(&std::process::id().to_string()), "{said}");
                // The operator reads this sentence. It must not read like a stack trace.
                for jargon in ["flock", "EWOULDBLOCK", "Errno", "LockError", "None"] {
                    assert!(
                        !said.contains(jargon),
                        "jargon reached the operator: {said}"
                    );
                }
            }
            other => panic!("a second hub was allowed to start: {other:?}"),
        }
    }

    #[test]
    fn releasing_the_lock_lets_the_next_hub_start() {
        // The restart case, and the reason this is an advisory lock rather than a pidfile: it has
        // to clear itself, including when the holder was killed rather than asked to stop.
        let d = tmp();
        let first = HubLock::acquire(d.path()).expect("first");
        drop(first);
        let _second =
            HubLock::acquire(d.path()).expect("a restart is not blocked by its own corpse");
    }

    #[test]
    fn a_missing_state_directory_is_created_rather_than_refused() {
        let d = tmp();
        let nested = d.path().join("not").join("there").join("yet");
        let lock = HubLock::acquire(&nested).expect("the directory is made");
        assert!(lock.path().exists());
    }

    #[test]
    fn the_lock_file_is_not_readable_by_anyone_else() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp();
        let lock = HubLock::acquire(d.path()).expect("locks");
        let mode = std::fs::metadata(lock.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "mode {:o} lets others read the state dir's lock",
            mode & 0o777
        );
    }
}

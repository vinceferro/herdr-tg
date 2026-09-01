//! The hub's half of the watchdog contract.
//!
//! `deploy/herdr-tg-watchdog.sh` watches one file and buzzes the operator's phone when it stops
//! being touched. It shares no code and no process with this binary — that is the point of it, and
//! it means the two are joined only by the file and by the contract written at the top of that
//! script. This module is that contract, in code, so the two cannot drift apart silently.
//!
//! # The contract, and why each clause is load-bearing
//!
//! **Stamped only from the live work loop.** A stamp emitted by a detached timer would keep
//! reporting health for a process whose dispatcher had wedged — a hub that is dead to the operator
//! and healthy to the watchdog. So [`Heartbeat::stamp`] takes `&self` and is called from the loop
//! itself; there is deliberately no `spawn`-and-forget helper here to reach for.
//!
//! **Updated in place, never removed.** Not even on a clean shutdown. An absent file used to mean
//! "the hub has never run", which is also what a tidy shutdown produces — and that reading turned
//! the alarm off permanently while looking exactly like correct silence. The watchdog now arms
//! itself the first time it sees this file, so deleting it is an *alarm*, not a disarm. Deleting
//! it here would fire that alarm on every stop.
//!
//! **Never on a tmpfs.** The path comes from `$XDG_STATE_HOME` or `~/.local/state`, both of which
//! survive a reboot. A stamp under `/run` would be wiped at boot, and a wiped stamp on a
//! never-yet-armed watchdog is silence forever.
//!
//! # The file's contents are not the signal
//!
//! The watchdog reads the modification time and nothing else. The word written here is for a human
//! running `cat` at 3am — and for the case the mtime will never be able to express: a hub that is
//! running, stamping, and *unable to reach Telegram* because a second copy holds the long-poll.
//! [`HubHealth`] cannot say that yet, because nothing in this binary can observe it; the variant
//! arrives with the 409 counter in the hub's own loop. The file writes a word rather than nothing
//! so that there is somewhere for that answer to go when it exists.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What the hub was able to say about itself at the moment it stamped.
///
/// One variant today, and that is the honest count: the only thing this build can truthfully
/// assert is that it round-tripped the Bot API. The second variant — alive but unable to deliver,
/// because another copy holds the update slot — arrives with the 409 counter in the hub's own
/// loop, since nothing here can observe that state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubHealth {
    /// Long-polling and dispatching. The ordinary case.
    Serving,
}

impl HubHealth {
    /// The word written into the file. Plain words: a person reads this one.
    fn word(self) -> &'static str {
        match self {
            Self::Serving => "serving",
        }
    }
}

/// The file the watchdog watches.
#[derive(Clone, Debug)]
pub struct Heartbeat {
    path: PathBuf,
}

impl Heartbeat {
    /// Points at an explicit file. Tests use this; the binary uses [`Heartbeat::default_path`].
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/herdr-tg/hub.heartbeat`, else `~/.local/state/…`.
    ///
    /// The same derivation as the audit log and the routing state, and the same one the watchdog's
    /// unit hard-codes. The unit names it explicitly rather than relying on `$XDG_STATE_HOME`,
    /// because a `--user` service does not inherit the login shell's environment and the two would
    /// otherwise resolve differently — the hub stamping one file while the watchdog watched
    /// another, each of them correct and the pair useless.
    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("hub.heartbeat")
    }

    /// The file being stamped.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record that the loop is alive, right now.
    ///
    /// Call this from inside the dispatch loop, never from a timer that could outlive it.
    ///
    /// Written in place rather than through a temporary and a rename. The watchdog reads only the
    /// modification time, so a torn write cannot mislead it, and an in-place write cannot produce
    /// the instant of absence that a create-and-rename briefly can.
    pub fn stamp(&self, health: HubHealth) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, format!("{}\n", health.word()))
    }

    /// How long ago this was stamped, or `None` if it has never been stamped or cannot be read.
    ///
    /// Only for `herdr-tg doctor`. The watchdog does its own reading, on purpose: a hub that can
    /// answer "am I alive?" is answering the one question it is not a witness to.
    pub fn age(&self) -> Option<std::time::Duration> {
        let modified = fs::metadata(&self.path).ok()?.modified().ok()?;
        std::time::SystemTime::now().duration_since(modified).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    #[test]
    fn stamping_creates_the_directory_and_the_file() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("nested").join("hub.heartbeat"));
        hb.stamp(HubHealth::Serving).expect("stamps");
        assert!(hb.path().exists());
    }

    #[test]
    fn stamping_twice_updates_the_same_file_rather_than_replacing_it() {
        // The watchdog arms itself the first time it sees this file and treats its disappearance
        // as an alarm. A stamp that unlinked and recreated would open a window where the file is
        // absent — brief, but the watchdog runs every sixty seconds forever.
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        hb.stamp(HubHealth::Serving).expect("stamps");
        let first = fs::metadata(hb.path()).expect("metadata").ino_or_len();
        hb.stamp(HubHealth::Serving).expect("stamps again");
        let second = fs::metadata(hb.path()).expect("metadata").ino_or_len();
        assert_eq!(
            first, second,
            "the stamp replaced the file instead of updating it"
        );
    }

    #[test]
    fn the_default_path_is_the_one_the_watchdog_unit_names() {
        // Pinned against the literal in deploy/herdr-tg-watchdog.service. If either moves, the hub
        // stamps one file while the watchdog watches another and both look perfectly healthy.
        let d = tmp();
        // SAFETY-FREE: this test sets an env var, so it must not run beside another that reads it.
        // There is exactly one such test, and this is it.
        unsafe { std::env::set_var("XDG_STATE_HOME", d.path()) };
        let p = Heartbeat::default_path();
        unsafe { std::env::remove_var("XDG_STATE_HOME") };
        assert_eq!(p, d.path().join("herdr-tg").join("hub.heartbeat"));
    }

    #[test]
    fn a_never_stamped_heartbeat_has_no_age_rather_than_a_zero_one() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        assert_eq!(hb.age(), None);
        hb.stamp(HubHealth::Serving).expect("stamps");
        assert!(hb.age().expect("an age") < Duration::from_secs(5));
    }

    /// Same-inode check without pulling in a platform trait at the call site.
    trait InoOrLen {
        fn ino_or_len(&self) -> u64;
    }
    impl InoOrLen for fs::Metadata {
        fn ino_or_len(&self) -> u64 {
            use std::os::unix::fs::MetadataExt;
            self.ino()
        }
    }
}

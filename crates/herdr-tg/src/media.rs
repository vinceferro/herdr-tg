//! The hub's two directories for files — what the operator sends, and what an agent sends him —
//! and the rules that keep each a mailbox.
//!
//! Bytes never cross the wall on the wire: a frame is 64 KiB and a screenshot is a megabyte. What
//! he sends is fetched into `<state>/media/<project>/<address>/`, and the frame carries the PATH.
//! What an agent sends is copied by its adapter into `<state>/outbox/<project>/<address>/`, and
//! the frame carries the NAME. A wall bind-mounts each conversation's two directories at the same
//! path inside — media read-only, outbox read-write — so there is no translation anywhere, the
//! same rule the socket already follows (`docs/ATTACHING.md` §14).
//!
//! The outbox is the untrusted side. A wall can write anything there — a link of either kind to
//! the operator's secrets under a harmless name, a directory, a FIFO — and then name it. So the
//! hub never opens an outbox file by path: it walks the directories by descriptor, opens the name
//! under the last one following no symlink, and checks WHAT IT OPENED — regular, its own, with no
//! second name, under the ceiling — never the name ([`MediaStore::open_for_sending`]).
//!
//! Three things here are load-bearing and cheap to lose:
//!
//! * **The hub mints every path it writes.** Nothing a phone or Telegram said is ever a segment of
//!   one. The reported filename travels in the frame as data, and the extension comes from a short
//!   table keyed on a declared mime, or is absent.
//! * **Explicit modes.** The state directory is `0700` on the live box by history, not by code —
//!   it is made with `create_dir_all` and no mode wherever it is made — so these directories carry
//!   `0700` themselves and are re-checked on every use, and the files carry `0600`.
//! * **A bound.** Forty-eight hours and 256 MiB per tree, oldest first, or an operator who sends
//!   screenshots for a month fills a home disk with a mailbox nobody reads. On entries as well as
//!   bytes: a conversation's directory that has been empty for a shelf life goes too, because a
//!   lane is a worktree name and a dispatcher that makes and destroys worktrees would otherwise
//!   leave one directory per tree here for ever — and the walk that enforces the cap runs on the
//!   path of every file he sends.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::hub::Addr;

/// The most a bot may fetch from Telegram. Not ours to raise.
///
/// *"Bots can download files of up to 20MB in size"* — the Bot API page, read on 5 September
/// 2026. Twenty million rather than twenty mebibytes, because the page does not say which and the
/// lower number fails closed: a file between the two, refused here, is told "too big", which is
/// true; fetched, it would be refused by Telegram and told "the download failed, send it again",
/// which is a lie that sends him round again.
pub const FETCH_CEILING: u64 = 20_000_000;

/// The most a bot may upload. Not ours to raise either.
///
/// *"10 MB max size for photos, 50 MB for other files"* — the Bot API page, on sending files by
/// multipart. Fifty million rather than fifty mebibytes for the reason [`FETCH_CEILING`] is
/// twenty million: the page does not say which, and the lower number is the one that never tells
/// him a smaller file would have worked when it would not.
pub const SEND_CEILING: u64 = 50_000_000;

/// The most a bot may upload AS A PICTURE. Over it, a picture goes as a document instead.
pub const PHOTO_CEILING: u64 = 10_000_000;

/// How long a file he sent stays readable. The ask ledger's own 48 hours, not a second number:
/// a file stays at least as long as the question it answered can still be edited.
pub const SHELF_LIFE: Duration = Duration::from_secs(48 * 60 * 60);

/// The most one tree keeps. Twelve files at the fetch ceiling, or a couple of hundred phone
/// screenshots — more than a person sends in two days by accident, and small enough that a home
/// disk never notices it.
pub const TREE_CAP: u64 = 256 * 1024 * 1024;

/// What the capped writer refuses with. The caller does not match on it — see
/// [`Capped::hit_the_ceiling`] — so it is prose for a log line and nothing more.
const OVER_THE_CEILING: &str = "over the ceiling";

/// One of the two trees, and the rules for writing into it or reading out of it.
pub struct MediaStore {
    root: PathBuf,
    cap: u64,
    shelf_life: Duration,
    /// Whose files these are: this process's uid, and nothing in a tree is trusted unless it is
    /// owned by exactly that. Held rather than asked for each time so a test can make every file
    /// look like somebody else's — the only way to watch the ownership check bite without root.
    owner: AtomicU32,
}

/// Why a file in an outbox will not be sent. The words still go; §14.4 has his line for each.
#[derive(Debug, PartialEq, Eq)]
pub enum NotSendable {
    /// Over [`SEND_CEILING`], by the hub's own `fstat` on what it opened. Carries the size.
    TooBig(u64),
    /// Not a regular file the hub owns, reached without following a link, inside the outbox — or
    /// a name the hub will not open at all. The words are for the audit; he reads one sentence.
    NotAFile(String),
}

/// A file from an outbox, opened and checked, with the size the check read.
pub struct Opened {
    pub file: fs::File,
    pub size: u64,
}

/// What a sweep took away, for the log line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub files: usize,
    pub bytes: u64,
}

impl MediaStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cap: TREE_CAP,
            shelf_life: SHELF_LIFE,
            owner: AtomicU32::new(rustix::process::getuid().as_raw()),
        }
    }

    /// A smaller cap, so a test can watch the bound bite without writing a quarter of a gigabyte.
    #[cfg(test)]
    pub fn with_cap(mut self, cap: u64) -> Self {
        self.cap = cap;
        self
    }

    /// Require every file and directory to be owned by `uid` instead of by this process. Test-only:
    /// a test cannot `chown` a file to somebody else without root, so the one way to watch the
    /// ownership check refuse is to move the hub's idea of itself.
    #[cfg(test)]
    pub fn expect_owner(&self, uid: u32) {
        self.owner.store(uid, Ordering::SeqCst);
    }

    fn owner(&self) -> u32 {
        self.owner.load(Ordering::SeqCst)
    }

    /// Whether `name` is one the hub will open at all — the address rules of `docs/ATTACHING.md`
    /// §4, which a lane name already meets: one segment, not empty, at most 64 bytes, not `.` or
    /// `..`, no `/` or `\`, no control character.
    ///
    /// Checked BEFORE the disk is touched, so nothing an agent names can ever be a path: a name
    /// with a `/` in it, handed to `openat` under the outbox, would walk wherever it said.
    pub fn name_is_openable(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= crate::hub::MAX_LANE
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\\')
            && !name.chars().any(char::is_control)
    }

    /// Open `name` in the conversation's own directory of this tree, and check what was opened.
    ///
    /// The whole walk is by descriptor. Each directory — the tree's root, the project's, the
    /// conversation's — is opened under the one before it with `O_DIRECTORY | O_NOFOLLOW` and
    /// checked as [`own_private_dir`] checks it: a directory, not a link, this hub's own, `0700`.
    /// The file is then opened under the last with `O_NOFOLLOW`, so a link at that name is refused
    /// by the kernel rather than followed; and with `O_NONBLOCK`, because a FIFO opened for
    /// reading without it parks this process until something writes into it, which a wall can
    /// arrange and never do. Then `fstat` on the descriptor: a regular file, owned by this hub,
    /// with no second name anywhere on this filesystem, under `ceiling`. Nothing here resolves a
    /// path a wall wrote and opens it afterwards — a wall can swap the name between the two, so
    /// every check is on the thing that was opened.
    pub fn open_for_sending(
        &self,
        addr: &Addr,
        name: &str,
        ceiling: u64,
    ) -> Result<Opened, NotSendable> {
        use rustix::fs::{CWD, FileType, Mode, OFlags, fstat, openat};
        if !Self::name_is_openable(name) {
            return Err(NotSendable::NotAFile(
                "the name is not one the hub will open".to_owned(),
            ));
        }
        let dir_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let not_a_file = |what: &str, e: rustix::io::Errno| {
            NotSendable::NotAFile(format!("{what}: {}", io::Error::from(e)))
        };
        let root = openat(CWD, &self.root, dir_flags, Mode::empty())
            .map_err(|e| not_a_file("the outbox could not be opened", e))?;
        self.is_own_private_dir(&root, "the outbox")?;
        let project = openat(&root, addr.project.as_str(), dir_flags, Mode::empty())
            .map_err(|e| not_a_file("the project's outbox could not be opened", e))?;
        self.is_own_private_dir(&project, "the project's outbox")?;
        let lane = addr.lane.as_ref().map_or("-", |l| l.as_str());
        let dir = openat(&project, lane, dir_flags, Mode::empty())
            .map_err(|e| not_a_file("the conversation's outbox could not be opened", e))?;
        self.is_own_private_dir(&dir, "the conversation's outbox")?;
        let fd = openat(
            &dir,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|e| match e {
            rustix::io::Errno::LOOP => {
                NotSendable::NotAFile("it is a link, and the hub follows none".to_owned())
            }
            // `open` refuses a socket with ENXIO before there is a descriptor to `fstat`; the
            // arm below that names a socket is for a kernel that answers differently.
            rustix::io::Errno::NXIO => {
                NotSendable::NotAFile("it is a socket, not a regular file".to_owned())
            }
            e => not_a_file("it could not be opened", e),
        })?;
        let st = fstat(&fd).map_err(|e| not_a_file("it could not be examined", e))?;
        let kind = match FileType::from_raw_mode(st.st_mode) {
            FileType::RegularFile => None,
            FileType::Directory => Some("a directory"),
            FileType::Symlink => Some("a link"),
            FileType::Fifo => Some("a pipe"),
            FileType::Socket => Some("a socket"),
            FileType::CharacterDevice | FileType::BlockDevice => Some("a device"),
            _ => Some("not a regular file"),
        };
        if let Some(kind) = kind {
            return Err(NotSendable::NotAFile(format!(
                "it is {kind}, not a regular file"
            )));
        }
        if st.st_uid != self.owner() {
            return Err(NotSendable::NotAFile(format!(
                "it is owned by uid {}, not by this hub (uid {}); nothing in this design wrote it",
                st.st_uid,
                self.owner()
            )));
        }
        // One name, and it is this one. A hard link passes every check above it — it IS a regular
        // file, it IS owned by whoever owns the inode, and its directory entry IS under the
        // descriptor this hub opened — so without this the "inside the outbox" rule would be
        // enforced on the NAME after all, and `link` needs no read permission on its source: a
        // file a wall may name through a read-only mount but must not read is linkable, and would
        // then be read out of here and handed to Telegram. An adapter copies bytes in, and a copy
        // has exactly one name, so nothing this design writes is refused by it.
        if st.st_nlink != 1 {
            return Err(NotSendable::NotAFile(format!(
                "it has another name outside this outbox ({} in all), and the hub sends only \
                 files written for it",
                st.st_nlink
            )));
        }
        let size = u64::try_from(st.st_size).unwrap_or(u64::MAX);
        if size > ceiling {
            return Err(NotSendable::TooBig(size));
        }
        Ok(Opened {
            file: fs::File::from(fd),
            size,
        })
    }

    /// The descriptor-side twin of [`own_private_dir`]: the same three requirements, on a
    /// directory already opened with `O_NOFOLLOW`, so a link swapped in after the open cannot
    /// change the answer.
    fn is_own_private_dir(
        &self,
        fd: &impl std::os::fd::AsFd,
        what: &str,
    ) -> Result<(), NotSendable> {
        let st = rustix::fs::fstat(fd).map_err(|e| {
            NotSendable::NotAFile(format!(
                "{what} could not be examined: {}",
                io::Error::from(e)
            ))
        })?;
        if rustix::fs::FileType::from_raw_mode(st.st_mode) != rustix::fs::FileType::Directory {
            return Err(NotSendable::NotAFile(format!("{what} is not a directory")));
        }
        if st.st_uid != self.owner() {
            return Err(NotSendable::NotAFile(format!(
                "{what} is owned by uid {}, not by this hub (uid {})",
                st.st_uid,
                self.owner()
            )));
        }
        let mode = st.st_mode & 0o777;
        if mode != 0o700 {
            return Err(NotSendable::NotAFile(format!(
                "{what} is mode {mode:o}, not 700; anyone on this machine could put a file in it"
            )));
        }
        Ok(())
    }

    /// Make this tree's own root, and require it to be what the hub would have made.
    ///
    /// Run once when the hub starts, because `docs/ATTACHING.md` §14.1 says so to whoever is
    /// writing a dispatcher — and because a root somebody else owns, or one left 0755, is a fact
    /// worth reading in the first line of the journal rather than discovering on the first
    /// screenshot he sends. Refused rather than repaired: widening or chowning a directory this
    /// hub did not make is not a thing it may do on its own.
    pub fn make_the_tree(&self) {
        if let Err(e) = own_private_dir(&self.root, self.owner()) {
            tracing::error!(
                error = %e, root = %self.root.display(),
                "this directory is not one this hub will use, so no file will go in or out of it \
                 until somebody fixes it"
            );
        }
    }

    /// The conversation's own directory, made if absent and checked whether or not it was.
    ///
    /// `-` is the segment for the project's own voice. It cannot collide with a lane, because the
    /// address rules refuse `-` as a lane name before it can ever become one.
    ///
    /// Every segment is required to be a directory and not a link, owned by this process, and
    /// mode `0700` — on every use, not only when made. A wrapper that starts a wall makes these
    /// itself so it has something to mount, and a directory somebody else made, or made wider,
    /// is one the hub writes his screenshot into for anybody on the box to read.
    pub fn dir_for(&self, addr: &Addr) -> io::Result<PathBuf> {
        let project = self.root.join(addr.project.as_str());
        let dir = project.join(addr.lane.as_ref().map_or("-", |l| l.as_str()));
        for seg in [self.root.as_path(), project.as_path(), dir.as_path()] {
            own_private_dir(seg, self.owner())?;
        }
        Ok(dir)
    }

    /// The path a file about to be written gets: the moment and a random suffix, so a listing of
    /// the directory is arrival order, and an extension from [`extension_for`] or none.
    pub fn mint(dir: &Path, mime: Option<&str>) -> PathBuf {
        let mut random = [0u8; 4];
        // A clock and four bytes of randomness are the whole name. If the randomness is not
        // there, the clock alone still cannot be made to collide by anything he sends, and the
        // exclusive create below refuses the one-in-four-billion case rather than overwriting.
        let _ = getrandom::fill(&mut random);
        let stamp = utc_stamp(SystemTime::now());
        let mut name = format!(
            "{stamp}-{:02x}{:02x}{:02x}{:02x}",
            random[0], random[1], random[2], random[3]
        );
        if let Some(ext) = mime.and_then(extension_for) {
            name.push('.');
            name.push_str(ext);
        }
        dir.join(name)
    }

    /// Open a minted path for writing: `0600`, and refusing to overwrite anything.
    ///
    /// Exclusive creation also refuses a link left at that name, which nothing should be able to
    /// leave in a directory only the hub writes — but the mount is the wrapper's promise, and this
    /// is the hub's.
    pub fn create(path: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }

    /// Take away what has aged out, then the oldest until the tree is under its cap.
    ///
    /// Age is the `mtime` stat — the moment the hub, or the wall, wrote the file — never a name
    /// anybody chose. Only regular files are counted or removed: a link or anything else in here
    /// was not written by this code, and is left where it is with a line in the log.
    pub fn sweep(&self, now: SystemTime) -> Swept {
        // Which directories were already quiet, read BEFORE a byte is removed. Taking a file away
        // touches the directory it was in, and taking a conversation's directory away touches its
        // project's, so a tree read afterwards looks busy everywhere this sweep has just been.
        let quiet = self.quiet_directories(now);
        let mut swept = Swept::default();
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for path in files_under(&self.root) {
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                tracing::warn!(
                    path = %path.display(), root = %self.root.display(),
                    "something in this tree is not a regular file; left alone"
                );
                continue;
            }
            let written = meta.modified().unwrap_or(now);
            let age = now.duration_since(written).unwrap_or_default();
            if age > self.shelf_life {
                take_away(&path, meta.len(), &mut swept);
                continue;
            }
            kept.push((written, meta.len(), path));
        }
        // Oldest first, by the moment the hub wrote it. The name is not consulted: names are the
        // hub's own here, but sorting by them would be one edit away from sorting by a name a
        // wall chose, on the day the outbox reuses this.
        kept.sort_by_key(|(written, _, _)| *written);
        let mut total: u64 = kept.iter().map(|(_, size, _)| size).sum();
        for (_, size, path) in kept {
            if total <= self.cap {
                break;
            }
            take_away(&path, size, &mut swept);
            total -= size;
        }
        if swept.files > 0 {
            // Not silent: a directory shrinking is a thing someone reading an incident afterwards
            // has to be able to account for.
            tracing::info!(
                files = swept.files, bytes = swept.bytes, root = %self.root.display(),
                "removed files that had aged out or pushed this tree past its cap"
            );
        }
        forget(quiet);
        swept
    }

    /// Take away the directory of a conversation that has been empty for a shelf life, and then
    /// its project's when that is empty too.
    ///
    /// The cap bounds BYTES. A lane is a git worktree name, so a dispatcher that makes and
    /// destroys worktrees left one directory per tree here for ever — nothing over the cap, and
    /// nothing the cap could ever see. That matters twice: it is a slow leak of entries, and the
    /// walk that enforces the cap runs on the path of every file he sends, so what it costs has
    /// to be bounded by what is live rather than by everything that ever was.
    ///
    /// **Empty is not enough; it has to have been empty for the shelf life.** A wall mounts a
    /// conversation's directory at the same path, and a mount holds the directory it was made
    /// from: removing one under a running wall leaves that wall writing into a directory the hub
    /// can no longer see. Forty-eight hours with nothing written or taken away is the evidence
    /// this conversation is not the one a wall is using. When it is wrong anyway the failure is
    /// visible rather than silent — the hub finds no such file, the words go with the line saying
    /// the file did not come, and the journal names the directory — and restarting the wall,
    /// which remakes its mounts, is the fix.
    fn quiet_directories(&self, now: SystemTime) -> Vec<PathBuf> {
        let is_quiet = |p: &Path| -> bool {
            fs::symlink_metadata(p).is_ok_and(|m| {
                m.file_type().is_dir()
                    && m.modified()
                        .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > self.shelf_life)
            })
        };
        let mut out = Vec::new();
        for project in dirs_in(&self.root) {
            // Deepest first, so a project emptied by this same pass is tried after the
            // conversations that were in it rather than before them.
            out.extend(dirs_in(&project).into_iter().filter(|c| is_quiet(c)));
            if is_quiet(&project) {
                out.push(project);
            }
        }
        out
    }
}

/// Remove directories that should be empty by now, saying nothing about the ones that are not.
///
/// A directory with anything left in it, or one a wall still has mounted, answers with an error
/// and is simply kept — this is a tidy-up, and declining to remove is always the safe answer.
fn forget(quiet: Vec<PathBuf>) {
    for dir in quiet {
        match fs::remove_dir(&dir) {
            Ok(()) => tracing::info!(
                path = %dir.display(),
                "removed a directory nothing has used for a shelf life"
            ),
            Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(e) => tracing::debug!(
                error = %e, path = %dir.display(), "left a directory that would not go"
            ),
        }
    }
}

/// The directories directly inside `p`, and nothing else.
fn dirs_in(p: &Path) -> Vec<PathBuf> {
    fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Make one segment if it is absent, then require it to be what the hub would have made: a
/// directory and not a link, this process's own, mode `0700`. Anything else is refused by name.
///
/// The mode is re-asserted after creation rather than trusted: `DirBuilder::mode` is subject to
/// the umask, and a mode a wrapper set wider is exactly what the check on the next line exists to
/// catch — so the directory the hub makes itself must pass the same check.
fn own_private_dir(seg: &Path, owner: u32) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(seg) {
        Ok(()) => {
            fs::set_permissions(seg, fs::Permissions::from_mode(0o700))?;
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(seg)?;
    let name = seg.display();
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "{name} is a link, not a directory; the hub will not write through it"
        )));
    }
    if !meta.is_dir() {
        return Err(io::Error::other(format!("{name} is not a directory")));
    }
    if meta.uid() != owner {
        return Err(io::Error::other(format!(
            "{name} is owned by uid {}, not by this hub (uid {owner}); nothing here made it",
            meta.uid()
        )));
    }
    let mode = meta.mode() & 0o777;
    if mode != 0o700 {
        return Err(io::Error::other(format!(
            "{name} is mode {mode:o}, not 700; anyone on this machine could read what he sent"
        )));
    }
    Ok(())
}

/// Every entry three levels down — `<project>/<address>/<file>` — as paths. Nothing above or
/// below that depth is a file the hub wrote.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = |p: &Path| -> Vec<PathBuf> {
        fs::read_dir(p)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    for project in dirs_in(root) {
        for address in dirs_in(&project) {
            out.extend(entries(&address));
        }
    }
    out
}

fn take_away(path: &Path, size: u64, swept: &mut Swept) {
    match fs::remove_file(path) {
        Ok(()) => {
            swept.files += 1;
            swept.bytes += size;
        }
        Err(e) => tracing::warn!(
            error = %e, path = %path.display(),
            "could not remove a file that should have been swept"
        ),
    }
}

/// The extension a declared mime earns, or none.
///
/// A courtesy so an agent's own tools can tell a picture from a text file without being told; a
/// short table so that nothing a sender declares can name an extension the hub did not choose.
/// Keyed on the exact string, so `image/png; anything` earns nothing.
pub fn extension_for(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/markdown" => "md",
        "text/csv" => "csv",
        "application/json" => "json",
        "audio/ogg" => "ogg",
        "audio/mpeg" => "mp3",
        "video/mp4" => "mp4",
        "application/zip" => "zip",
        _ => return None,
    })
}

/// What a photo is, read off the extension of Telegram's OWN `file_path` in the `getFile` answer.
///
/// Telegram declares no mime for a photo — it re-encodes what the phone sent — and the only thing
/// it says about the bytes is the name it stored them under, `photos/file_123.jpg`. So the mime
/// on a photo's frame is read from that, per file, rather than assumed once for every photo ever.
pub fn mime_from_telegram_path(file_path: &str) -> Option<&'static str> {
    let ext = file_path.rsplit_once('.')?.1;
    Some(match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// A destination that counts what it takes and refuses the byte past the ceiling.
///
/// Telegram's size on a message is optional, and the client library reads an absent one as four
/// gigabytes. So the ceiling is enforced on the STREAM, on what actually arrives, whatever was
/// reported — and a file that reaches the cap mid-transfer is cut off there, not written whole
/// and measured afterwards.
///
/// Writes go straight to the file, blocking, from inside the async write: a chunk is at most tens
/// of kilobytes into the page cache, which is the same cost the audit line already pays, and the
/// alternative is a runtime feature this crate does not otherwise turn on.
pub struct Capped {
    file: fs::File,
    written: u64,
    cap: u64,
    /// Set the moment a write is refused for the ceiling, so the caller can tell the ceiling from
    /// a full disk without matching on the error's prose — which passes through the download
    /// library on the way back and comes out wrapped.
    over: bool,
}

impl Capped {
    pub fn new(file: fs::File, cap: u64) -> Self {
        Self {
            file,
            written: 0,
            cap,
            over: false,
        }
    }

    /// Bytes taken so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Whether a write was refused for the ceiling, as opposed to the disk.
    pub fn hit_the_ceiling(&self) -> bool {
        self.over
    }
}

impl tokio::io::AsyncWrite for Capped {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        use std::io::Write as _;
        if self.written + buf.len() as u64 > self.cap {
            self.over = true;
            return Poll::Ready(Err(io::Error::other(OVER_THE_CEILING)));
        }
        match self.file.write_all(buf) {
            Ok(()) => {
                self.written += buf.len() as u64;
                Poll::Ready(Ok(buf.len()))
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        use std::io::Write as _;
        Poll::Ready(self.file.flush())
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

/// `yyyymmdd-hhmmss`, UTC, dependency-free.
///
/// The civil-date arithmetic is the standard days-to-date conversion (Howard Hinnant's); a date
/// crate is a supply-chain decision for a file name.
fn utc_stamp(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hub_proto::ProjectId;

    fn mode_of(p: &Path) -> u32 {
        fs::symlink_metadata(p).expect("exists").mode() & 0o777
    }

    fn own() -> Addr {
        Addr::project_itself(ProjectId::new("p-9f3a1c2e5b7d"))
    }

    #[test]
    fn the_media_directory_is_created_0700_and_files_0600() {
        // The state directory is 0700 on the live box by history — `create_dir_all` and no mode,
        // everywhere it is made — so a directory that merely inherited would be a promise nothing
        // enforces. His screenshot of his bank is in here; every segment carries the mode itself.
        let tmp = tempfile::tempdir().expect("tmp");
        let store = MediaStore::new(tmp.path().join("media"));
        let dir = store.dir_for(&own()).expect("made");
        assert_eq!(
            dir,
            tmp.path().join("media").join("p-9f3a1c2e5b7d").join("-"),
            "the project's own voice is the `-` segment"
        );
        for seg in [
            tmp.path().join("media"),
            tmp.path().join("media/p-9f3a1c2e5b7d"),
            dir.clone(),
        ] {
            assert_eq!(
                mode_of(&seg),
                0o700,
                "{} is {:o}, not 0700",
                seg.display(),
                mode_of(&seg)
            );
        }
        let path = MediaStore::mint(&dir, Some("image/png"));
        drop(MediaStore::create(&path).expect("created"));
        assert_eq!(
            mode_of(&path),
            0o600,
            "{} is {:o}, not 0600",
            path.display(),
            mode_of(&path)
        );
        // And a second open of the same name is refused, never an overwrite.
        assert!(
            MediaStore::create(&path).is_err(),
            "the same minted name was opened twice"
        );
    }

    #[test]
    fn a_segment_that_is_not_the_hubs_own_directory_is_refused_by_name() {
        // A wrapper makes the conversation's directory before the wall starts, and can make it
        // wrong: a link somewhere else, or a mode the whole box can read. Either is refused on
        // every use, naming the segment, and never written into.
        let tmp = tempfile::tempdir().expect("tmp");
        let store = MediaStore::new(tmp.path().join("media"));
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("dir");
        // The root as the hub makes it, so the refusal below is about the segment under test.
        fs::DirBuilder::new()
            .mode(0o700)
            .create(tmp.path().join("media"))
            .expect("dir");
        std::os::unix::fs::symlink(&elsewhere, tmp.path().join("media/p-9f3a1c2e5b7d"))
            .expect("link");
        let refused = store
            .dir_for(&own())
            .expect_err("a link was accepted as the directory");
        assert!(
            refused.to_string().contains("p-9f3a1c2e5b7d"),
            "the refusal does not name the segment: {refused}"
        );
        assert!(
            !elsewhere.join("-").exists(),
            "the hub made a directory through the link"
        );

        fs::remove_file(tmp.path().join("media/p-9f3a1c2e5b7d")).expect("unlink");
        let wide = tmp.path().join("media/p-9f3a1c2e5b7d");
        fs::create_dir_all(&wide).expect("dir");
        fs::set_permissions(&wide, fs::Permissions::from_mode(0o755)).expect("chmod");
        let refused = store
            .dir_for(&own())
            .expect_err("a world-readable segment was accepted");
        assert!(
            refused.to_string().contains("p-9f3a1c2e5b7d") && refused.to_string().contains("755"),
            "the refusal does not say what is wrong: {refused}"
        );
    }

    #[test]
    fn old_media_is_removed_oldest_first_past_the_cap() {
        // A mailbox, not an archive. Past the cap the OLDEST goes first, by the hub's own mtime and
        // never by a name; past the shelf life a file goes whatever the cap says; and what the
        // sweep does not touch is exactly the newest that still fit.
        let tmp = tempfile::tempdir().expect("tmp");
        let store = MediaStore::new(tmp.path().join("media")).with_cap(1_000);
        let dir = store.dir_for(&own()).expect("made");
        let now = SystemTime::now();
        let aged = |secs: u64| now - Duration::from_secs(secs);
        // Five of 300 bytes, a minute apart, and one tiny one from three days ago. Names are
        // chosen so that "oldest" and "alphabetically first" disagree: a sweep that sorted by name
        // would take the wrong ones.
        let files = [
            ("zz-oldest", 300, 300),
            ("aa-second", 300, 240),
            ("mm-third", 300, 180),
            ("bb-fourth", 300, 120),
            ("yy-newest", 300, 60),
            ("stale", 10, 3 * 24 * 60 * 60),
        ];
        for (name, size, age) in files {
            let p = dir.join(name);
            fs::write(&p, vec![b'x'; size]).expect("write");
            fs::File::open(&p)
                .expect("open")
                .set_modified(aged(age))
                .expect("mtime");
        }
        let swept = store.sweep(now);
        let left: Vec<String> = {
            let mut v: Vec<String> = fs::read_dir(&dir)
                .expect("dir")
                .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            left,
            vec!["bb-fourth", "mm-third", "yy-newest"],
            "the wrong files survived the sweep: {left:?} (swept {swept:?})"
        );
        assert_eq!(
            swept,
            Swept {
                files: 3,
                bytes: 610
            }
        );
        // Idempotent: a second sweep of a tree under its cap takes nothing.
        assert_eq!(store.sweep(now), Swept::default());
    }

    #[test]
    fn a_minted_name_is_the_moment_a_random_suffix_and_an_extension_only_from_the_table() {
        // `<yyyymmdd>-<hhmmss>-<8 hex>` and, for a mime the table knows, its extension. Nothing
        // else can reach the name: a declared mime that is not exactly a table key — including
        // one with a path or a parameter tacked on — earns no extension at all.
        let dir = Path::new("/state/media/p/-");
        let p = MediaStore::mint(dir, Some("image/png"));
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(
            name.len() == "20260905-231455-9f3a1c2e.png".len()
                && name.ends_with(".png")
                && name[..8].chars().all(|c| c.is_ascii_digit())
                && &name[8..9] == "-"
                && name[9..15].chars().all(|c| c.is_ascii_digit())
                && &name[15..16] == "-"
                && name[16..24].chars().all(|c| c.is_ascii_hexdigit()),
            "{name}"
        );
        assert_eq!(p.parent(), Some(dir));
        for odd in [
            "application/x-sh",
            "image/png; charset=../../x",
            "IMAGE/PNG",
            "../../../etc/passwd",
            "",
        ] {
            let p = MediaStore::mint(dir, Some(odd));
            let name = p.file_name().unwrap().to_str().unwrap();
            assert_eq!(name.len(), 24, "{odd:?} named an extension: {name}");
            assert!(!name.contains('/') && !name.contains('.'), "{name}");
        }
        assert_eq!(utc_stamp(UNIX_EPOCH), "19700101-000000");
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(1_788_650_095)),
            "20260905-231455"
        );
    }

    #[test]
    fn a_photos_mime_is_read_off_telegrams_own_path_and_only_off_that() {
        assert_eq!(
            mime_from_telegram_path("photos/file_123.jpg"),
            Some("image/jpeg")
        );
        assert_eq!(
            mime_from_telegram_path("photos/file_123.JPEG"),
            Some("image/jpeg")
        );
        assert_eq!(mime_from_telegram_path("documents/file_9.pdf"), None);
        assert_eq!(mime_from_telegram_path("noext"), None);
    }

    #[tokio::test]
    async fn a_download_past_the_ceiling_is_cut_off_where_the_ceiling_is() {
        // The reported size is optional and reads as four gigabytes when absent, so the only
        // check that always runs is this one, on the bytes as they arrive.
        use tokio::io::AsyncWriteExt as _;
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("f");
        let mut w = Capped::new(MediaStore::create(&path).expect("created"), 100);
        w.write_all(&[b'a'; 60]).await.expect("under");
        let refused = w
            .write_all(&[b'b'; 60])
            .await
            .expect_err("wrote past the ceiling");
        assert!(w.hit_the_ceiling(), "{refused}");
        assert_eq!(
            w.written(),
            60,
            "the count moved for bytes that were refused"
        );
        assert_eq!(fs::metadata(&path).expect("file").len(), 60);
    }

    #[test]
    fn a_second_name_for_a_file_is_not_one_the_outbox_may_send() {
        // A hard link passes every other check on this list — it IS a regular file, it IS owned by
        // whoever owns the inode, and its directory entry IS inside the outbox — so without this
        // one the "inside the outbox" rule is enforced on the NAME after all. A wall that may name
        // a file through a read-only mount but not read it can still `link` it here and have the
        // hub read it out and hand it to Telegram: `link` needs no read permission on the source.
        let tmp = tempfile::tempdir().expect("tmp");
        let store = MediaStore::new(tmp.path().join("outbox"));
        let dir = store.dir_for(&own()).expect("made");
        let elsewhere = tmp.path().join("not-for-a-phone");
        fs::write(&elsewhere, b"the other name for these bytes").expect("write");
        fs::hard_link(&elsewhere, dir.join("shot.png")).expect("link");
        let refused = store
            .open_for_sending(&own(), "shot.png", 1_000)
            .err()
            .expect("a file with another name outside the outbox was opened and would be sent");
        let NotSendable::NotAFile(why) = refused else {
            panic!("refused for the wrong reason: {refused:?}");
        };
        assert!(why.contains("another name"), "{why}");
        // And a file the adapter actually copied in, which has exactly one name, still goes.
        fs::write(dir.join("chart.png"), b"copied in by an adapter").expect("write");
        assert!(store.open_for_sending(&own(), "chart.png", 1_000).is_ok());
    }

    #[test]
    fn a_conversation_directory_nothing_is_using_stops_costing_an_entry() {
        // A lane is a git worktree name, so a dispatcher that makes and destroys worktrees left
        // one directory per tree per worktree here for ever: the documented bound is on BYTES, and
        // an entry the cap cannot see is an entry nothing ever takes away. The walk is on the path
        // of every screenshot he sends, so what it costs has to be bounded too.
        let tmp = tempfile::tempdir().expect("tmp");
        let store = MediaStore::new(tmp.path().join("media"));
        let now = SystemTime::now();
        let long_ago = now - Duration::from_secs(30 * 24 * 60 * 60);
        for lane in ["w1", "w2", "w3"] {
            let addr = Addr::lane_of(
                ProjectId::new("p-9f3a1c2e5b7d"),
                hub_proto::LaneId::new(lane),
            );
            let dir = store.dir_for(&addr).expect("made");
            let f = dir.join("old");
            fs::write(&f, b"aged out").expect("write");
            fs::File::open(&f)
                .expect("open")
                .set_modified(long_ago)
                .expect("mtime");
            fs::File::open(&dir)
                .expect("open the directory")
                .set_modified(long_ago)
                .expect("the directory's own mtime");
        }
        // The project's own directory has been touched by each of those, so it is aged here for
        // the same reason: this is a repository whose worktrees all went a month ago.
        fs::File::open(store.root.join("p-9f3a1c2e5b7d"))
            .expect("open the project's directory")
            .set_modified(long_ago)
            .expect("mtime");
        let swept = store.sweep(now);
        assert_eq!(swept.files, 3, "{swept:?}");
        let left = under(&store.root);
        assert!(
            left.is_empty(),
            "three worktrees that will never come back are still costing a directory each: {left:?}"
        );
    }

    /// Every path under a tree, deepest first, as strings relative to it — for reading what a
    /// sweep left behind.
    fn under(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(p) = stack.pop() {
            let Ok(rd) = fs::read_dir(&p) else { continue };
            for e in rd.filter_map(Result::ok) {
                let path = e.path();
                out.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                );
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        out.sort();
        out
    }
}

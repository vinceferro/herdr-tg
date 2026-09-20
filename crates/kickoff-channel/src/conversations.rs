//! The channel's own space for conversations — outside every repo.
//!
//! A conversation used to be a function of the directory a session launched in: the bridge walked
//! up from that directory to `<repo>/.kickoff/hub.token` and presented what it found. That put a
//! credential inside every adopted repository, and on this box ten of thirteen would commit it.
//! Here a conversation is a ROW the operator minted at a terminal, and its secret lives under the
//! hub's own state directory, one directory per conversation, where no `git add` can reach it.
//!
//! ```text
//! <state>/                                  0700, re-asserted
//!   conversations/<id>/secret               0600   the bytes a bridge presents
//!   conversations/<id>/title                0600   display only, written by whoever holds the slot
//!   by-repo/<sha256(canonical repo)[..16]>  0600   one line: the id a repo defaults to
//!   grants/<seed id>/NNN.vacant | NNN.taken 0600   one line: a room's id
//! ```
//!
//! Nothing here changes how the hub decides who is connecting. It still hashes the bytes a bridge
//! presented and compares them against `token_sha256` in the registry; the bytes are COPIED into
//! this tree, never rotated, so the hub cannot tell — and never needs to tell — which file they were
//! read out of. The hub reads exactly one thing here, the optional `title`, and reads it once
//! per run: the first time it composes the conversation's name, and never again while it runs.
//!
//! # Two rules that are load-bearing, and both were paid for elsewhere in this repo
//!
//! * **The mode is step zero.** Measured: the service runs with umask 0022, and a bare
//!   `create_dir_all` under that umask makes a `755` directory. The state directory is 0700 on the
//!   live box by history, not by code. Everything written here is a credential, so every directory
//!   is made 0700 and RE-ASSERTED after creation — `DirBuilder::mode` is subject to the umask, and
//!   an existing directory keeps whatever it had.
//! * **A conversation id becomes a path segment.** It is shape-refused at every door, AND the
//!   canonical containment of the target is asserted immediately before every write — because the
//!   check and the write live in different functions, and this repo has shipped a defect of exactly
//!   that shape before.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use hub_proto::ProjectId;

/// The three trees under the channel home.
pub const CONVERSATIONS: &str = "conversations";
pub const BY_REPO: &str = "by-repo";
pub const GRANTS: &str = "grants";

/// The two files a conversation's directory may hold.
pub const SECRET: &str = "secret";
pub const TITLE: &str = "title";

/// How many vacant rooms a seed may hold at once. Refilled only at a terminal.
pub const BOOK: usize = 16;

/// Is this string a conversation id, and therefore safe to become a path segment?
///
/// `p-` and twelve hex characters is what the registry mints from a canonical repo path; `c-` and
/// twelve random hex characters is a room. Nothing else — not a dot, not a slash, not an
/// uppercase hex digit, not a byte over 0x7f — is a name this module will join onto a path.
pub fn is_conversation_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 14
        && (b[0] == b'p' || b[0] == b'c')
        && b[1] == b'-'
        && b[2..]
            .iter()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// The `by-repo` key for a repository: the first sixteen hex characters of the SHA-256 of its
/// canonical path — the same bytes `Registry::enrol` hashes to mint the project's id, so the link
/// and the id are derived from one fact.
pub fn repo_key(repo: &Path) -> String {
    let canonical = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    sha256_hex(canonical.as_os_str().as_encoded_bytes())[..16].to_owned()
}

/// A fresh room id: `c-` and twelve random hex characters. Random rather than derived, because a
/// room has no path of its own to derive from and a counter is the shape that once recycled a dead
/// agent's topic.
pub fn mint_room_id() -> Option<ProjectId> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).ok()?;
    Some(ProjectId::new(format!("c-{}", hex(&bytes))))
}

/// One slot in a seed's book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub number: u32,
    pub taken: bool,
    pub room: ProjectId,
}

/// The channel home, and the three trees under it.
#[derive(Clone, Debug)]
pub struct ChannelHome {
    root: PathBuf,
}

impl ChannelHome {
    /// The home at a given root. Touches nothing.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn conversations(&self) -> PathBuf {
        self.root.join(CONVERSATIONS)
    }

    pub fn by_repo(&self) -> PathBuf {
        self.root.join(BY_REPO)
    }

    pub fn grants(&self) -> PathBuf {
        self.root.join(GRANTS)
    }

    /// Step zero: the home and every tree under it, 0700, before any credential lands.
    ///
    /// Called by every writer before it writes, not once at install: a fresh box gets its state
    /// directory from the hub's own start, which makes it with no mode at all, so the first verb
    /// that puts a credential here has to be the thing that closes it.
    pub fn prepare(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.conversations(),
            self.by_repo(),
            self.grants(),
        ] {
            private_state_dir(&dir)?;
        }
        Ok(())
    }

    /// The directory of one conversation, shape-checked and made if absent. Never a link, never
    /// anybody else's, never wider than 0700.
    fn conversation_dir(&self, id: &ProjectId, make: bool) -> io::Result<PathBuf> {
        let dir = self.conversations().join(segment(id)?);
        if make {
            self.prepare()?;
            private_state_dir(&dir)?;
        }
        Ok(dir)
    }

    /// Where a conversation's secret is. Shape-checked; touches nothing.
    pub fn secret_path(&self, id: &ProjectId) -> io::Result<PathBuf> {
        Ok(self.conversation_dir(id, false)?.join(SECRET))
    }

    /// Write a conversation's secret, making its directory.
    pub fn write_secret(&self, id: &ProjectId, secret: &str) -> io::Result<PathBuf> {
        let dir = self.conversation_dir(id, true)?;
        // Immediately before the write, and in the same function as it: the shape check above
        // cannot see a directory that was swapped for a link after it was made.
        let path = contained(&self.conversations(), &dir.join(SECRET))?;
        write_private_file(&path, secret.as_bytes())?;
        Ok(path)
    }

    /// A conversation's secret, or `None` when there is none.
    ///
    /// Read the way it is written: through no link. A secret read through a link out of the tree
    /// is a credential nobody minted here, presented as if somebody had.
    pub fn read_secret(&self, id: &ProjectId) -> io::Result<Option<String>> {
        let dir = self.conversation_dir(id, false)?;
        if !dir.exists() {
            return Ok(None);
        }
        let path = contained(&self.conversations(), &dir.join(SECRET))?;
        match read_private_file(&path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Take a whole conversation away: its secret, its title, and the directory. For a room a
    /// grant minted and could not save a row for — a credential with no row is one the hub can
    /// never admit and nothing on the box would ever explain.
    pub fn remove_conversation(&self, id: &ProjectId) -> io::Result<()> {
        let dir = self.conversation_dir(id, false)?;
        if !dir.exists() {
            return Ok(());
        }
        for name in [SECRET, TITLE] {
            let path = contained(&self.conversations(), &dir.join(name))?;
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        let dir = contained(&self.conversations(), &dir)?;
        match fs::remove_dir(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Take a conversation's secret away again. For rolling back a rotation that failed halfway.
    pub fn remove_secret(&self, id: &ProjectId) -> io::Result<()> {
        let dir = self.conversation_dir(id, false)?;
        if !dir.exists() {
            return Ok(());
        }
        let path = contained(&self.conversations(), &dir.join(SECRET))?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Bind a repository to the conversation it defaults to.
    ///
    /// The id goes INTO the file as data, so it is shape-checked here too: a link naming `../x`
    /// would be joined onto a path by whoever reads it next.
    pub fn link_repo(&self, repo: &Path, id: &ProjectId) -> io::Result<PathBuf> {
        let id = segment(id)?;
        self.prepare()?;
        let path = contained(&self.by_repo(), &self.by_repo().join(repo_key(repo)))?;
        write_private_file(&path, format!("{id}\n").as_bytes())?;
        Ok(path)
    }

    /// The conversation a repository defaults to, if a link has been written — and only when
    /// what the link names is a conversation id, because it is about to become a path segment.
    pub fn linked(&self, repo: &Path) -> io::Result<Option<ProjectId>> {
        let path = self.by_repo().join(repo_key(repo));
        match read_private_file(&path) {
            Ok(s) => {
                let s = s.trim();
                if is_conversation_id(s) {
                    Ok(Some(ProjectId::new(s)))
                } else {
                    Err(io::Error::other(format!(
                        "{} names {s:?}, which is not a conversation",
                        path.display()
                    )))
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The display title whoever holds a conversation wrote for it, if there is one it can show.
    ///
    /// Display only, read once at topic creation, and shape-refused on the clauses a lane name is:
    /// non-empty, no control character, no more characters than a topic title takes. Anything
    /// else — a missing file, a link out of the tree, an unshowable string — is `None`, and the
    /// caller falls back to the registry's own name. Nothing in the hub ever writes one.
    pub fn title_of(&self, id: &ProjectId) -> Option<String> {
        let dir = self.conversation_dir(id, false).ok()?;
        let path = contained(&self.conversations(), &dir.join(TITLE)).ok()?;
        let raw = read_private_file(&path).ok()?;
        let title = raw.trim();
        let showable = !title.is_empty()
            && title.chars().count() <= MAX_TITLE
            && !title.chars().any(char::is_control);
        showable.then(|| title.to_owned())
    }

    /// The book of one seed, shape-checked and made if asked.
    fn book_dir(&self, seed: &ProjectId, make: bool) -> io::Result<PathBuf> {
        let dir = self.grants().join(segment(seed)?);
        if make {
            self.prepare()?;
            private_state_dir(&dir)?;
        }
        Ok(dir)
    }

    /// Every slot in a seed's book, in number order.
    pub fn slots(&self, seed: &ProjectId) -> io::Result<Vec<Slot>> {
        let dir = self.book_dir(seed, false)?;
        let mut out = Vec::new();
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for e in rd {
            let e = e?;
            let name = e.file_name();
            let name = name.to_string_lossy();
            let (number, taken) = match name.rsplit_once('.') {
                Some((n, "vacant")) => (n, false),
                Some((n, "taken")) => (n, true),
                _ => continue,
            };
            let Ok(number) = number.parse::<u32>() else {
                continue;
            };
            let room = read_private_file(&contained(&self.grants(), &e.path())?)?;
            let room = room.trim();
            if !is_conversation_id(room) {
                return Err(io::Error::other(format!(
                    "{} names {room:?}, which is not a conversation",
                    e.path().display()
                )));
            }
            out.push(Slot {
                number,
                taken,
                room: ProjectId::new(room),
            });
        }
        out.sort_by_key(|s| s.number);
        Ok(out)
    }

    /// Take a VACANT slot out of a seed's book — one a grant minted and then could not save a
    /// row for, or one found naming a room the list has never heard of. A taken slot is never
    /// removed: its number is spent, because the room it named may have a topic and a history.
    pub fn remove_slot(&self, seed: &ProjectId, number: u32) -> io::Result<()> {
        let dir = self.book_dir(seed, false)?;
        let path = contained(&self.grants(), &dir.join(format!("{number:03}.vacant")))?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Put a room into the next slot of a seed's book. A number is never reused: a slot that was
    /// taken stays taken, because the room it named may have a topic and a history.
    pub fn add_slot(&self, seed: &ProjectId, room: &ProjectId) -> io::Result<Slot> {
        let room_id = segment(room)?;
        let dir = self.book_dir(seed, true)?;
        let number = self.slots(seed)?.last().map_or(0, |s| s.number + 1);
        let path = contained(&self.grants(), &dir.join(format!("{number:03}.vacant")))?;
        write_private_file(&path, format!("{room_id}\n").as_bytes())?;
        Ok(Slot {
            number,
            taken: false,
            room: room.clone(),
        })
    }
}

/// Longest topic title Telegram will take. The registry's own figure, kept in step by its test.
const MAX_TITLE: usize = 48;

/// An id as a path segment, or a refusal naming it — before anything is joined.
fn segment(id: &ProjectId) -> io::Result<&str> {
    let s = id.as_str();
    if is_conversation_id(s) {
        Ok(s)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{s:?} is not a conversation id, so it cannot name a directory"),
        ))
    }
}

/// Where `path` really is, and a refusal unless that is inside `tree`.
///
/// The parent is canonicalised — through every link — and must sit under the canonical tree; the
/// last segment is then joined back on unresolved, so a link AT the target is replaced by the
/// write rather than followed by it. Asked immediately before every write and every read, in the
/// same function, so a directory swapped for a link after the shape check cannot move the target.
fn contained(tree: &Path, path: &Path) -> io::Result<PathBuf> {
    let tree = tree.canonicalize()?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("a path with no parent"))?
        .canonicalize()?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("a path with no name"))?;
    if !parent.starts_with(&tree) {
        return Err(io::Error::other(format!(
            "{} would land outside {}, at {}; refused",
            path.display(),
            tree.display(),
            parent.display()
        )));
    }
    Ok(parent.join(name))
}

/// A file only this user can read, written whole: a temporary beside it, then one rename, so a
/// kill mid-write leaves the old bytes or the new and never half of each.
fn write_private_file(path: &Path, body: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(body)?;
        f.flush()?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // Re-asserted rather than trusted: `.mode()` applies only at creation, and a file that was
    // once wider stays wider through every rewrite.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// Read a file that must be a regular file and not a link — a credential read through a link is
/// one nobody minted here.
fn read_private_file(path: &Path) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "{} is a link, and a credential is never read through one",
            path.display()
        )));
    }
    if !meta.is_file() {
        return Err(io::Error::other(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    fs::read_to_string(path)
}

/// Make one directory of the hub's state, or tighten one that is already there, to 0700.
///
/// The seven places that make the state directory used to call `create_dir_all` bare, and under
/// the service's umask that is 755 — the state directory on the live box is 0700 by history, not
/// by code. Made 0700 here, and re-asserted after: `DirBuilder::mode` is subject to the umask, and
/// an existing directory keeps whatever it had. A directory this process does not own, or a link
/// standing where a directory should be, is refused by name rather than tightened: chowning or
/// writing through something somebody else put there is not a thing this hub may do on its own.
pub fn private_state_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        private_state_dir(parent)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(dir)?;
    let name = dir.display();
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "{name} is a link, not a directory; nothing here is written through one"
        )));
    }
    if !meta.is_dir() {
        return Err(io::Error::other(format!("{name} is not a directory")));
    }
    let me = rustix::process::getuid().as_raw();
    if meta.uid() != me {
        return Err(io::Error::other(format!(
            "{name} is owned by uid {}, not by this user (uid {me}); nothing here made it",
            meta.uid()
        )));
    }
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let after = fs::symlink_metadata(dir)?.mode() & 0o777;
        if after != 0o700 {
            return Err(io::Error::other(format!(
                "{name} is mode {after:o} and could not be made 700; anyone on this machine could \
                 read a credential put there"
            )));
        }
    }
    Ok(())
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

    /// The mode a path has on disk, as three octal digits.
    fn mode_of(p: &Path) -> u32 {
        fs::symlink_metadata(p).expect("metadata").mode() & 0o777
    }

    /// Run `f` with the umask the service runs under, and put the old one back after.
    fn under_umask_022<T>(f: impl FnOnce() -> T) -> T {
        let old = rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o022));
        let out = f();
        rustix::process::umask(old);
        out
    }

    fn id(s: &str) -> ProjectId {
        ProjectId::new(s)
    }

    #[test]
    fn the_channel_home_and_everything_under_it_is_created_0700_before_any_secret_lands() {
        // Measured on the live box: the service runs with umask 0022, and a bare `create_dir_all`
        // under it makes 755. Every directory here holds a credential, so the mode has to come from
        // this code and not from history — and it has to be re-asserted, because `DirBuilder::mode`
        // is subject to the umask and an existing directory keeps whatever it had.
        let d = tempfile::tempdir().expect("tmp");
        let root = d.path().join("herdr-tg");
        let home = ChannelHome::at(&root);
        let written =
            under_umask_022(|| home.write_secret(&id("p-0123456789ab"), "s3cret")).expect("writes");
        for dir in [
            root.clone(),
            home.conversations(),
            home.by_repo(),
            home.grants(),
            home.conversations().join("p-0123456789ab"),
        ] {
            assert_eq!(
                mode_of(&dir),
                0o700,
                "{} is mode {:o}, and it holds credentials",
                dir.display(),
                mode_of(&dir)
            );
        }
        assert_eq!(mode_of(&written), 0o600);

        // An existing wider directory is TIGHTENED, not trusted: the hub's own start makes the
        // state directory with no mode at all, so on a fresh box it is 755 before any verb runs.
        let wide = d.path().join("wide");
        under_umask_022(|| fs::create_dir_all(&wide)).expect("mkdir");
        assert_eq!(
            mode_of(&wide),
            0o755,
            "the fixture is not the shape it is about"
        );
        ChannelHome::at(&wide).prepare().expect("prepares");
        assert_eq!(
            mode_of(&wide),
            0o700,
            "an existing 755 home was left as it was"
        );
        assert_eq!(mode_of(&wide.join(CONVERSATIONS)), 0o700);
    }

    #[test]
    fn a_conversation_id_that_is_not_a_valid_segment_is_refused_at_every_door() {
        // An id becomes a path segment. Every door that joins one onto a path refuses anything but
        // the exact shape the registry mints — before touching the disk, so a refused id leaves no
        // directory behind and cannot name one outside the tree.
        let d = tempfile::tempdir().expect("tmp");
        let home = ChannelHome::at(d.path().join("home"));
        let elsewhere = d.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("mkdir");
        let bad = [
            "".to_owned(),
            "p-".to_owned(),
            "p-0123456789a".to_owned(),
            "p-0123456789abc".to_owned(),
            "x-0123456789ab".to_owned(),
            "p-0123456789AB".to_owned(),
            "p-0123456789ag".to_owned(),
            "p-0123456789a/".to_owned(),
            "..".to_owned(),
            "../elsewhere/x".to_owned(),
            "p-0123456789ab/../../elsewhere/x".to_owned(),
            "p-0123456789a\n".to_owned(),
            format!("../{}", elsewhere.display()),
        ];
        for raw in &bad {
            let wrong = id(raw);
            assert!(!is_conversation_id(raw), "{raw:?} passed the shape check");
            assert!(
                home.write_secret(&wrong, "s").is_err(),
                "write_secret took {raw:?} as a path segment"
            );
            assert!(
                home.read_secret(&wrong).is_err(),
                "read_secret took {raw:?} as a path segment"
            );
            assert!(
                home.secret_path(&wrong).is_err(),
                "secret_path took {raw:?} as a path segment"
            );
            assert!(
                home.remove_secret(&wrong).is_err(),
                "remove_secret took {raw:?} as a path segment"
            );
            assert!(
                home.title_of(&wrong).is_none(),
                "title_of took {raw:?} as a path segment"
            );
            assert!(
                home.slots(&wrong).is_err(),
                "slots took {raw:?} as a path segment"
            );
            assert!(
                home.add_slot(&wrong, &id("c-0123456789ab")).is_err(),
                "add_slot took {raw:?} as a seed"
            );
            assert!(
                home.add_slot(&id("p-0123456789ab"), &wrong).is_err(),
                "add_slot took {raw:?} as a room"
            );
            assert!(
                home.link_repo(&elsewhere, &wrong).is_err(),
                "link_repo took {raw:?} as a conversation"
            );
        }
        assert!(
            fs::read_dir(&elsewhere).expect("readable").next().is_none(),
            "something landed outside the home"
        );
        let conversations = home.conversations();
        let made: Vec<_> = fs::read_dir(&conversations)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(
            made.is_empty(),
            "a refused id still made something: {made:?}"
        );
        assert!(is_conversation_id("p-0123456789ab"));
        assert!(is_conversation_id("c-abcdef012345"));
    }

    #[test]
    fn a_write_that_would_escape_the_conversation_directory_is_refused_before_it_happens() {
        // The shape check and the write live in different functions. So the write itself asks the
        // filesystem where the target really is, and refuses when the answer is outside the tree —
        // a conversation directory replaced by a link to somewhere else passes every shape check
        // and would otherwise put a credential wherever the link points.
        let d = tempfile::tempdir().expect("tmp");
        let home = ChannelHome::at(d.path().join("home"));
        home.prepare().expect("prepares");
        let elsewhere = d.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("mkdir");
        let room = id("c-0123456789ab");
        std::os::unix::fs::symlink(&elsewhere, home.conversations().join(room.as_str()))
            .expect("link");

        assert!(
            home.write_secret(&room, "s3cret").is_err(),
            "a secret was written through a link out of the tree"
        );
        assert!(
            !elsewhere.join(SECRET).exists(),
            "the secret landed outside the home"
        );
        assert!(
            home.read_secret(&room).is_err(),
            "a secret was read through a link out of the tree"
        );
        assert!(home.title_of(&room).is_none());

        // The same for a seed's book, and for the by-repo tree.
        let seed = id("p-0123456789ab");
        std::os::unix::fs::symlink(&elsewhere, home.grants().join(seed.as_str())).expect("link");
        assert!(home.add_slot(&seed, &room).is_err());
        assert!(
            fs::read_dir(&elsewhere)
                .expect("readable")
                .all(|e| e.expect("entry").file_name() == "nothing"),
            "something landed outside the home"
        );
        fs::remove_dir_all(home.by_repo()).expect("rm");
        std::os::unix::fs::symlink(&elsewhere, home.by_repo()).expect("link");
        assert!(home.link_repo(&elsewhere, &seed).is_err());
        assert!(
            fs::read_dir(&elsewhere).expect("readable").next().is_none(),
            "a link was written outside the home"
        );
    }

    #[test]
    fn a_secret_is_written_whole_and_read_back_and_a_link_names_its_conversation() {
        let d = tempfile::tempdir().expect("tmp");
        let home = ChannelHome::at(d.path().join("home"));
        let repo = d.path().join("repo");
        fs::create_dir_all(&repo).expect("mkdir");
        let seed = id("p-0123456789ab");
        home.write_secret(&seed, "the-bytes").expect("writes");
        assert_eq!(
            home.read_secret(&seed).expect("reads").as_deref(),
            Some("the-bytes")
        );
        home.link_repo(&repo, &seed).expect("links");
        assert_eq!(home.linked(&repo).expect("reads"), Some(seed.clone()));
        assert_eq!(home.linked(&d.path().join("other")).expect("reads"), None);
        // Rewritten in place: a rotation replaces the bytes and keeps the mode.
        home.write_secret(&seed, "newer").expect("rewrites");
        assert_eq!(
            home.read_secret(&seed).expect("reads").as_deref(),
            Some("newer")
        );
        assert_eq!(mode_of(&home.secret_path(&seed).expect("path")), 0o600);
        home.remove_secret(&seed).expect("removes");
        assert_eq!(home.read_secret(&seed).expect("reads"), None);
    }

    #[test]
    fn a_book_numbers_its_slots_and_never_reuses_one() {
        let d = tempfile::tempdir().expect("tmp");
        let home = ChannelHome::at(d.path().join("home"));
        let seed = id("p-0123456789ab");
        let a = home.add_slot(&seed, &id("c-000000000001")).expect("slot");
        let b = home.add_slot(&seed, &id("c-000000000002")).expect("slot");
        assert_eq!((a.number, b.number), (0, 1));
        // A dispatcher takes a slot by renaming it; the number is spent for good.
        let dir = home.grants().join(seed.as_str());
        fs::rename(dir.join("000.vacant"), dir.join("000.taken")).expect("take");
        let c = home.add_slot(&seed, &id("c-000000000003")).expect("slot");
        assert_eq!(c.number, 2);
        let slots = home.slots(&seed).expect("slots");
        assert_eq!(
            slots
                .iter()
                .map(|s| (s.number, s.taken))
                .collect::<Vec<_>>(),
            vec![(0, true), (1, false), (2, false)]
        );
        assert_eq!(slots[0].room, id("c-000000000001"));
    }

    #[test]
    fn a_title_is_shown_only_when_a_topic_can_carry_it() {
        // Display only, shape-refused on the same clauses as a lane name: an unreadable or
        // unshowable title falls back to the registry's, and nothing here ever writes one.
        let d = tempfile::tempdir().expect("tmp");
        let home = ChannelHome::at(d.path().join("home"));
        let room = id("c-0123456789ab");
        home.write_secret(&room, "s").expect("writes");
        let title = home.conversations().join(room.as_str()).join(TITLE);
        assert_eq!(home.title_of(&room), None, "no file is no title");
        fs::write(&title, "Customer onboarding\n").expect("write");
        assert_eq!(home.title_of(&room).as_deref(), Some("Customer onboarding"));
        for bad in ["", "   \n", "two\nlines", "tab\tin it", &"x".repeat(200)] {
            fs::write(&title, bad).expect("write");
            assert_eq!(
                home.title_of(&room),
                None,
                "a title the topic cannot carry was shown: {bad:?}"
            );
        }
    }
}

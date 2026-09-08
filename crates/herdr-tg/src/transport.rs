//! The door a bridge arrives at, and who the kernel says came through it. Nothing about meaning.
//!
//! # Why this is not in `hub.rs`
//!
//! Every rule the hub has — one claim per conversation, admission, delivery, eviction — was written
//! against a [`tokio::net::UnixStream`], so proving any of them needed a socket file on disk, a
//! kernel to give it credentials, and a second process to dial it. That made the cheap tests
//! expensive and the expensive ones the only kind. It also left the second transport this project
//! has already promised to consider (a gateway, `docs/CAPABILITIES.md` OPEN 4) with nowhere to be:
//! there was no name for "a connection" that was not the name of a Unix socket.
//!
//! So: above this seam a connection is bytes ([`ByteStream`]) plus an answer to *who is that*
//! ([`ConnectionIdentity`]), handed over together as [`Accepted`]. Below it, `AF_UNIX` from one
//! user to the same user is the only thing that exists, and [`LocalSocket`] is all of it.
//!
//! This module knows nothing about claims, secrets, projects or lanes, and must not learn — the
//! whole value of the seam is that the identity question can be answered a different way later
//! without the gates above having to be rewritten to ask it.
//!
//! # The identity is the kernel's answer, never the wire's
//!
//! `SO_PEERCRED` is read off the connection itself. A bridge's `hello` also carries a pid, and it
//! is a number the bridge chose: believing it would let a bridge make an incumbent look dead and
//! take its project. The two are compared for the journal and nothing else.
//!
//! # The trap: a pid means nothing outside the namespace that minted it
//!
//! `SO_PEERCRED` translates the peer's pid into the *reading* process's pid namespace, and a peer
//! that has no pid there is reported as **zero**. A hub running inside a pid namespace its bridges
//! are outside of would therefore be told fence 0 for every connection it accepts; `/proc/0` is not
//! a directory, so [`fence_is_alive`] would answer "gone" about every incumbent, and the
//! evict-a-corpse rule would evict the whole fleet, one live bridge after another, for ever.
//!
//! So a peer the kernel will not put a process behind is not admitted at all: [`identify`] refuses
//! it and [`LocalSocket::accept`] drops it. That is the fail-closed half — a connection nobody can
//! fence is one the single-claim rule cannot hold, and refusing it one line at a time in the
//! journal beats accepting it and evicting the whole fleet in a loop.
//!
//! It is only half, and the other half has half landed. What DID land: a run holds a numbered
//! lease on its address, and an incumbent the hub decides is gone is now ended by a kick down its
//! own connection instead of being silently overwritten — so the evicted task stops rather than
//! going on draining into a conversation its successor owns. What did NOT land: the hub still asks
//! `/proc` whether an incumbent is alive at all, in `hub::Hub::claim_the_address`, and it asks it
//! about the pid THIS file put on the connection. So the operating constraint is unchanged and
//! still load-bearing — the hub and its bridges must share a pid namespace, which on this machine
//! they do: both are the operator's own processes on his own box. The one repair that must NOT be
//! made is trusting `hello.pid` instead, which is exactly the number the paragraph above says
//! cannot be trusted.
//!
//! The uid half is unaffected: a foreign user is still refused, because 0 is not this user either.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};

/// Who is on the other end of a connection, as the transport that accepted it can prove.
///
/// **Opaque, and that is a safety property rather than a style.** An identity nobody proved is
/// exactly what the gates above this seam exist to refuse; while the fields were public, any file
/// in the crate could write down a uid and a pid and hand them to `serve_connection` as fact. The
/// only mints are in this file, and the only one that ships in the binary the operator runs is
/// [`identify`], which asks the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionIdentity(Identity);

/// The kinds of peer there are.
///
/// One variant, because there is one transport. It is an enum rather than a struct because the
/// questions the gates ask — "may this peer be here", "what does the single-claim rule fence on" —
/// have a different answer over a gateway, and a second variant is how that arrives without every
/// gate learning what a uid is. Private, so adding one is a change to this file, which is the
/// point of the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    /// A process on this machine, over `AF_UNIX`, according to the kernel.
    LocalPeer {
        uid: u32,
        /// The process on the other end of THIS connection — not the pid the bridge says it is.
        pid: u32,
    },
}

impl ConnectionIdentity {
    /// This process, as a peer. What a connection made in memory has to be given, because a pipe
    /// has no credentials to read: the caller states the identity instead of the kernel proving it.
    ///
    /// `cfg(test)` and the sealed inner type are the enforcement of that sentence, together: the
    /// gates above are built to refuse an identity nobody proved, so no way to state one may exist
    /// in the binary the operator runs — and with the fields private, `cfg(test)` on the mints is
    /// the whole of it.
    #[cfg(test)]
    pub fn this_process() -> Self {
        Self(Identity::LocalPeer {
            uid: our_uid(),
            pid: std::process::id(),
        })
    }

    /// A peer that is NOT this user, whoever this user is.
    ///
    /// Built off [`Self::this_process`] and changed by one rather than spelled out: which uid this
    /// process runs as is this file's business, and a test that named a number would be a test
    /// about the box it ran on. It lives here rather than beside the hub's tests because minting an
    /// identity is the transport's job even when a test is what needs one.
    #[cfg(test)]
    pub fn another_user() -> Self {
        let Self(Identity::LocalPeer { uid, pid }) = Self::this_process();
        Self(Identity::LocalPeer {
            uid: uid.wrapping_add(1),
            pid,
        })
    }

    /// This user, behind a process that has ended — what the evict-a-corpse rule is about, and the
    /// one thing a connection from this process can never be over a real socket.
    #[cfg(test)]
    pub fn this_user_behind_a_dead_process(gone: u32) -> Self {
        let Self(Identity::LocalPeer { uid, .. }) = Self::this_process();
        Self(Identity::LocalPeer { uid, pid: gone })
    }

    /// Gate 1: may this peer be here at all?
    ///
    /// Mode 0600 on the socket already implies it. Reading the credential back means the check
    /// survives someone's permissions mistake, and it is the transport's question rather than the
    /// hub's — over a gateway it would not be a uid comparison at all.
    pub fn is_this_user(&self) -> bool {
        match self.0 {
            Identity::LocalPeer { uid, .. } => uid == our_uid(),
        }
    }

    /// What the single-claim rule fences on: the number that says whether the incumbent still
    /// exists. Today that is the peer's pid, and [`fence_is_alive`] is how it is asked.
    pub fn fence(&self) -> u32 {
        match self.0 {
            Identity::LocalPeer { pid, .. } => pid,
        }
    }

    /// Can this peer open a path this hub can open?
    ///
    /// The outbox is named to an adapter as an absolute directory, so a peer that does not share
    /// this filesystem must be offered none: it would copy a file into a place the hub will never
    /// look and report the send as done. A local peer shares it; a gateway would not.
    pub fn shares_this_filesystem(&self) -> bool {
        match self.0 {
            Identity::LocalPeer { .. } => true,
        }
    }

    /// The peer's uid for a journal line, and for nothing else.
    ///
    /// A refusal is written down with the number it was about, or an incident is unreadable. It is
    /// deliberately not a getter: no gate may branch on this, because the next variant has no uid
    /// to give and every caller that branched on one would then be silently wrong.
    pub fn uid_for_the_log(&self) -> u32 {
        match self.0 {
            Identity::LocalPeer { uid, .. } => uid,
        }
    }
}

/// Is a fence still a process on this machine?
///
/// The evict-a-corpse rule depends on this being a fact rather than a hope. `/proc/<pid>` is the
/// fact; a signal-0 probe would answer "yes" for a pid this user does not own.
pub fn fence_is_alive(fence: u32) -> bool {
    Path::new(&format!("/proc/{fence}")).exists()
}

/// This process's uid, for comparison against a peer's.
fn our_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// A connection, in the two parts the semantics above need: something to read and write, and who
/// is on the other end of it.
///
/// `Send + 'static` because the writer half moves into a task of its own — a slow Telegram send
/// must never block reading the socket — and `Unpin` because the framing reads and writes it
/// directly rather than pinning it.
pub trait ByteStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> ByteStream for T {}

/// One accepted connection: the bytes, and the identity the transport proved for them.
///
/// They travel together on purpose. The pair is the only thing `serve_connection` is given, so
/// there is no path on which a stream is served without an identity having been established for it
/// first.
pub struct Accepted {
    pub stream: Box<dyn ByteStream>,
    pub who: ConnectionIdentity,
}

impl Accepted {
    /// Take a connection over anything that carries bytes, with the identity already established.
    pub fn over(stream: impl ByteStream, who: ConnectionIdentity) -> Self {
        Self {
            stream: Box::new(stream),
            who,
        }
    }
}

/// `/run/user/<uid>/kickoff/hub.sock`.
///
/// Derived on both sides, never configured. `XDG_RUNTIME_DIR` is not on kickoff's list of variables
/// that survive its `env -i` boundary, so a bridge started by a worker would not see it — the two
/// would derive different paths and neither would be wrong.
pub fn socket_path() -> PathBuf {
    let uid = our_uid();
    PathBuf::from(format!("/run/user/{uid}/kickoff")).join("hub.sock")
}

/// The only transport there is today: `AF_UNIX`, mode 0600, this user to this user.
pub struct LocalSocket {
    listener: UnixListener,
    /// Kept for [`LocalSocket::describe`]: a listener cannot be asked its own path once the file
    /// has been replaced under it, and the journal line is worth more than the saving.
    path: PathBuf,
}

impl LocalSocket {
    /// Bind the listener, with the directory and the socket locked down before anything can
    /// connect.
    ///
    /// A stale socket file from a previous run is removed first. That is safe because the hub lock
    /// is already held by this process — nothing else can be listening — and skipping it would make
    /// a crash require a manual `rm` from a keyboard.
    pub fn bind(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    /// Wait for the next connection whose peer the kernel will name.
    ///
    /// A connection the kernel will NOT name is dropped here and the wait resumes, rather than
    /// being handed up as an error. The caller's loop treats an error as the door being in trouble
    /// and backs off before trying again, so one unnameable peer — a process that hung up between
    /// the SYN and the accept is the ordinary way to get one — would slow every other bridge's dial
    /// for seconds, and a stream of them would hold the backoff at its ceiling with the Telegram
    /// half still running and nothing anywhere saying the bridges could no longer connect. That is
    /// this system's signature failure: silence that looks exactly like health.
    ///
    /// Dropping it is also the fail-closed answer: a peer that cannot be identified cannot be
    /// admitted, so there is nothing else to do with one.
    pub async fn accept(&self) -> std::io::Result<Accepted> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let who = identify(&stream);
            if let Some(accepted) = accepted_or_dropped(stream, who)? {
                return Ok(accepted);
            }
        }
    }

    /// What the journal says the hub is listening on: the door, and who may come through it.
    ///
    /// "It is up" and "it is up for the right people" are different facts, and only the second one
    /// is worth reading at three in the morning.
    pub fn describe(&self) -> String {
        format!("AF_UNIX at {}, this user only", self.path.display())
    }
}

/// Open a connection at a local door, say nothing, and close it.
///
/// The hub's own way of finding out whether its accept loop is still turning. It exists because
/// the two easy answers are both lies: that `bind` returned `Ok` once, minutes or days ago, and
/// that the socket file is on disk — a listener whose loop has ended leaves both of those looking
/// exactly as they do when everything is fine, and the kernel will still complete a `connect` into
/// its backlog. Only something coming out the far end is proof, so the caller watches its own
/// count of connections that came through and treats this as the thing that provokes one.
///
/// **It sends nothing, and waits for nothing.** A knock that wrote a byte would be a frame the hub
/// has to read, time out on and log; an immediate close is the one shape the far side already
/// treats as a non-event ("a connection was opened and never said anything"). It also costs the
/// far side nothing to serve, which matters for something that runs every forty-five seconds for
/// the life of the process.
pub async fn knock(path: &Path) -> std::io::Result<()> {
    // Dropped straight away, and named rather than `let _ =` so it cannot be mistaken for a
    // connection that is being kept.
    let opened = UnixStream::connect(path).await?;
    drop(opened);
    Ok(())
}

/// Read the credentials of whoever is on the other end of a connection.
///
/// See the module docs for what the pid means, and where it stops meaning it. A peer the kernel
/// gives no process for — pid zero, which is what a peer outside this process's pid namespace
/// reads as — is an ERROR rather than a fence of zero: there is no process to ask `/proc` about,
/// so an incumbent holding that fence would be judged dead the instant it was judged, and the
/// single-claim rule would hand its conversation to whoever dialled next, for ever.
fn identify(stream: &UnixStream) -> std::io::Result<ConnectionIdentity> {
    // tokio's own reader rather than rustix's: rustix returns the pid as a `NonZeroI32` filled
    // straight from `getsockopt` with nothing checking it, so the zero the kernel really can report
    // would be a niche violation — undefined behaviour in the one place this file exists to be
    // certain about. This one hands back a plain `Option`, and the zero is refused below.
    let cred = stream.peer_cred()?;
    let pid = match cred.pid() {
        Some(pid) if pid > 0 => pid as u32,
        _ => {
            return Err(std::io::Error::other(
                "the kernel named no process behind this connection",
            ));
        }
    };
    Ok(ConnectionIdentity(Identity::LocalPeer {
        uid: cred.uid(),
        pid,
    }))
}

/// What becomes of one connection the listener has just handed over: served, or dropped so the wait
/// can resume.
///
/// A decision rather than four lines inside [`LocalSocket::accept`], because it is the seam's one
/// piece of policy that no test could otherwise reach — a connected `AF_UNIX` peer on Linux always
/// has credentials, so "the kernel would not name it" cannot be manufactured over a real socket,
/// and a branch nothing can drive is a branch the next editor deletes.
///
/// The return type is the policy: `Ok(None)`, never `Err`. An `Err` here is the caller's signal
/// that the DOOR is in trouble, and it backs off before opening it again — so an unnameable peer
/// reported as an error would slow every other bridge's dial for seconds, and a stream of them
/// would hold that backoff at its ceiling with the Telegram half still running and nothing anywhere
/// saying the bridges could no longer connect. That is this system's signature failure: silence
/// that looks exactly like health.
fn accepted_or_dropped(
    stream: impl ByteStream,
    who: std::io::Result<ConnectionIdentity>,
) -> std::io::Result<Option<Accepted>> {
    match who {
        Ok(who) => Ok(Some(Accepted::over(stream, who))),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "a connection was dropped because the kernel would not say who opened it"
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// The whole of gate 1 in one place: the kernel's answer about the far end, taken off the
    /// connection rather than believed off the wire.
    #[tokio::test]
    async fn a_connection_from_this_process_is_accepted_as_this_user_and_carries_the_kernels_own_pid()
     {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub.sock");
        let socket = LocalSocket::bind(&path).expect("bind");

        let dial = tokio::spawn(async move {
            tokio::net::UnixStream::connect(&path)
                .await
                .expect("connect")
        });
        let accepted = socket.accept().await.expect("accept");
        let _client = dial.await.expect("dial");

        assert!(
            accepted.who.is_this_user(),
            "our own connection is our own user"
        );
        assert_eq!(accepted.who.fence(), std::process::id());
        assert!(fence_is_alive(accepted.who.fence()));
        assert!(accepted.who.shares_this_filesystem());
    }

    /// Mode 0600 on the socket and 0700 on its directory are the first gate, and they are set
    /// before anything can dial: a window in which they are not is a window in which another user
    /// connects.
    #[tokio::test]
    async fn the_socket_and_the_directory_above_it_are_readable_by_nobody_but_this_user() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("run").join("hub.sock");
        let _socket = LocalSocket::bind(&path).expect("bind");

        let sock_mode = std::fs::metadata(&path)
            .expect("stat socket")
            .permissions()
            .mode()
            & 0o777;
        let dir_mode = std::fs::metadata(path.parent().expect("parent"))
            .expect("stat dir")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(sock_mode, 0o600, "the socket");
        assert_eq!(dir_mode, 0o700, "the directory the socket is in");
    }

    /// A hub that was killed leaves its socket file behind. Requiring a human with a keyboard to
    /// `rm` it before the hub can start again is an outage nobody would be told about.
    #[tokio::test]
    async fn a_socket_file_left_behind_by_a_hub_that_died_does_not_stop_the_next_one_binding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub.sock");
        let stale = LocalSocket::bind(&path).expect("first bind");
        drop(stale);
        assert!(path.exists(), "the file a dead hub leaves behind");

        LocalSocket::bind(&path).expect("the next hub binds over it");
    }

    /// The one piece of policy in this file, and the failure it prevents is a quiet one: a peer the
    /// kernel will not name is DROPPED, and the wait resumes. It must never come back to the caller
    /// as an error, because the caller reads an error as the door being in trouble and sleeps —
    /// longer each time — before opening it again, so one such peer would slow every other bridge's
    /// dial and a stream of them would leave nothing able to connect with nothing saying so.
    ///
    /// Driven through the decision rather than a real socket because a connected `AF_UNIX` peer on
    /// Linux always has credentials: this branch cannot be reached from outside, which is exactly
    /// why it needs a test of its own rather than a comment.
    #[tokio::test]
    async fn a_peer_the_kernel_would_not_name_is_dropped_rather_than_reported_as_the_door_failing()
    {
        let (stream, _far_end) = tokio::io::duplex(64);
        let dropped = accepted_or_dropped(
            stream,
            Err(std::io::Error::other("the kernel named no process")),
        )
        .expect("a peer without credentials is not the door failing");
        assert!(
            dropped.is_none(),
            "a peer the kernel would not name was handed up to be served"
        );

        // And the ordinary peer still gets through, so the drop is a branch and not the rule.
        let (stream, _far_end) = tokio::io::duplex(64);
        let served = accepted_or_dropped(stream, Ok(ConnectionIdentity::this_process()))
            .expect("a peer the kernel named is not an error either")
            .expect("a peer the kernel named is served");
        assert!(served.who.is_this_user());
    }

    /// The evict-a-corpse rule is only as good as this answer being a fact.
    #[test]
    fn a_fence_whose_process_is_gone_is_not_alive_and_this_process_is() {
        assert!(fence_is_alive(std::process::id()));
        // No such pid, so the answer is false without racing a real process into existence.
        assert!(!fence_is_alive(u32::MAX));
    }

    /// The point of the seam: the semantics above it need bytes and an identity, not a socket. A
    /// duplex in memory is both, so a test of the hub need not put a file on disk.
    #[tokio::test]
    async fn an_in_memory_pipe_is_a_byte_stream_a_connection_can_be_accepted_over() {
        let (hub_side, mut bridge_side) = tokio::io::duplex(64 * 1024);
        let accepted = Accepted::over(hub_side, ConnectionIdentity::this_process());
        assert!(accepted.who.is_this_user());

        let (rx, mut tx) = tokio::io::split(accepted.stream);
        let sent =
            hub_proto::Envelope::new(hub_proto::FrameId::new("h-ping"), hub_proto::HubFrame::Ping);
        hub_proto::write_frame(&mut tx, &sent).await.expect("write");

        let mut reader = hub_proto::FrameReader::new(&mut bridge_side);
        let got = reader
            .next::<hub_proto::HubFrame>()
            .await
            .expect("read")
            .expect("a frame");
        assert_eq!(got.id.as_str(), "h-ping");
        drop(rx);
    }

    /// The heartbeat's own probe, from both sides: it comes through a door that is being answered,
    /// and it carries no bytes with it. A knock that said something would be a frame the far side
    /// has to wait on and log, forty-five seconds apart, forever.
    #[tokio::test]
    async fn a_knock_comes_through_an_open_door_and_carries_nothing_with_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub.sock");
        let socket = LocalSocket::bind(&path).expect("bind");

        let knocking = tokio::spawn(async move { knock(&path).await });
        let accepted = socket.accept().await.expect("the knock came through");
        knocking
            .await
            .expect("the knock finished")
            .expect("knocked");

        let (rx, _tx) = tokio::io::split(accepted.stream);
        let mut reader = hub_proto::FrameReader::new(rx);
        assert!(
            reader
                .next::<hub_proto::BridgeFrame>()
                .await
                .expect("a closed connection is not an error")
                .is_none(),
            "the knock said something; the far side now has a frame to time out on"
        );
    }

    /// A door that was never opened must fail the knock rather than look like a visit — this is
    /// the whole of the socket half of the watchdog contract.
    #[tokio::test]
    async fn a_knock_at_a_door_that_was_never_opened_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        knock(&dir.path().join("nothing-here.sock"))
            .await
            .expect_err("there is nothing to come through");
    }

    /// What the journal says the hub is listening on. A line that names the door and who may reach
    /// it is the difference between "it is up" and "it is up for the right people".
    #[tokio::test]
    async fn the_transport_says_what_it_is_listening_on_and_who_may_reach_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub.sock");
        let socket = LocalSocket::bind(&path).expect("bind");

        let said = socket.describe();
        assert!(said.starts_with("AF_UNIX at "), "{said}");
        assert!(said.contains(&path.display().to_string()), "{said}");
        assert!(said.ends_with(", this user only"), "{said}");
    }
}

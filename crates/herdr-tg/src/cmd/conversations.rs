//! `herdr-tg open <repo>`, `grant <repo> --rooms N`, `adopt-secrets [--apply]` and
//! `remove-repo-secret <repo>` — the terminal doors of the conversations redesign.
//!
//! Beside `enroll.rs`, which is the older door and still writes a secret into the repo. These
//! write a secret where the channel keeps one — under the hub's own state directory, outside
//! every repository — so there is nothing left in a working tree for a `git add` to find. The
//! four are argv-only for the reason `enroll` is: admission is the one thing no message can do,
//! and that boundary is only real if the way in is a keyboard.
//!
//! `open` and `grant` are gated on a PERSON being at the keyboard, not merely on being argv. The
//! precedent is the override flag on `enroll`, and it was paid for: the likeliest reader of one
//! of these refusals is an autonomous agent that was just told to run the command.
//!
//! `adopt-secrets` is the migration, and it obeys three rules the design names: it never routes
//! through `Registry::enrol`, which would re-derive every id and orphan every topic; it never
//! writes `projects.json`; and it is dry-run by default, one-shot, and idempotent. A crash halfway
//! leaves a second copy of a secret nobody is reading yet, and rollback is `rm -rf` of the new
//! tree.

use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::conversations::ChannelHome;
use crate::registry::{Registry, TOKEN_FILE};

/// What a person has to be for `open`, `grant` and `remove-repo-secret` to do anything.
fn nobody_is_typing(verb: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{verb} mints or removes a credential, and that is a thing a person has to choose: nothing \
         here was typed at a terminal. Nothing has been written. Run it again at a keyboard if you \
         mean it."
    )
}

/// `herdr-tg open <repo>`: a conversation for a repo, with nothing written into the repo.
pub(crate) fn open(repo: &Path) -> anyhow::Result<()> {
    open_in(
        &Registry::default_path(),
        repo,
        std::io::stdin().is_terminal(),
        &mut std::io::stdout(),
    )
}

/// The same, against a named registry and with "is a person typing this" passed in, so the door
/// can be tested without opening a real project into the operator's own state directory.
fn open_in(
    registry_path: &Path,
    repo: &Path,
    at_a_terminal: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    if !at_a_terminal {
        return Err(nobody_is_typing("open"));
    }
    let mut registry = Registry::load(registry_path);
    let (project, created) = registry.open(repo)?;
    writeln!(
        out,
        "{:<14} {}",
        if created { "opened" } else { "already open" },
        project.title
    )?;
    writeln!(out, "{:<14} {}", "conversation", project.id)?;
    writeln!(out, "{:<14} {}", "repo", project.repo.display())?;
    // Where the secret is, said as a place and not a path: the bridge finds it by the repo's own
    // link, and a dispatcher hands it on by id. Nothing here echoes the bytes.
    writeln!(
        out,
        "{:<14} kept by the channel, outside the repo. A session started in that folder finds it \
         on its own; a dispatcher names it with KICKOFF_HUB_CONVERSATION={}",
        "secret", project.id
    )?;
    match project.topic_id {
        Some(id) => writeln!(out, "{:<14} {id} (kept)", "topic")?,
        None => writeln!(
            out,
            "{:<14} created the first time its bridge connects",
            "topic"
        )?,
    }
    if !created {
        writeln!(
            out,
            "\nNothing was changed. To rotate its secret, enrol it again:  herdr-tg enroll  — a \
             rotation rewrites the secret where it already lives and puts nothing into the repo \
             that is not there already."
        )?;
    }
    Ok(())
}

/// `herdr-tg grant <repo> --rooms N`: N rooms for a seed, vacant until something connects as one.
pub(crate) fn grant(repo: &Path, rooms: usize) -> anyhow::Result<()> {
    grant_in(
        &Registry::default_path(),
        repo,
        rooms,
        std::io::stdin().is_terminal(),
        &mut std::io::stdout(),
    )
}

fn grant_in(
    registry_path: &Path,
    repo: &Path,
    rooms: usize,
    at_a_terminal: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    if !at_a_terminal {
        return Err(nobody_is_typing("grant"));
    }
    let mut registry = Registry::load(registry_path);
    let minted = registry.grant(repo, rooms)?;
    let home = ChannelHome::at(registry_path.parent().unwrap_or(Path::new(".")));
    let Some(seed) = minted.first().and_then(|room| {
        registry
            .all()
            .find(|p| p.repo == room.repo && !crate::registry::is_room(&p.id))
    }) else {
        writeln!(out, "nothing granted: no rooms were asked for")?;
        return Ok(());
    };
    let slots = home.slots(&seed.id)?;
    for room in &minted {
        let number = slots
            .iter()
            .find(|s| s.room == room.id)
            .map_or_else(|| "?".to_owned(), |s| format!("{:03}", s.number));
        writeln!(
            out,
            "{:<14} {} — slot {number}, {}",
            "room", room.id, room.title
        )?;
    }
    writeln!(
        out,
        "{:<14} {}",
        "book",
        home.grants().join(seed.id.as_str()).display()
    )?;
    let vacant = slots.iter().filter(|s| !s.taken).count();
    writeln!(
        out,
        "{:<14} {vacant} of {} vacant",
        "the book",
        crate::conversations::BOOK
    )?;
    writeln!(
        out,
        "\nA dispatcher takes a room by renaming its slot from .vacant to .taken — the rename is \
         exclusive, so two cannot take one — reads the room's id out of it, and starts the engine \
         with KICKOFF_HUB_CONVERSATION=<that id>. A line written to the room's title file names \
         its topic the first time it speaks; the rooms are hidden from /projects until then."
    )?;
    Ok(())
}

/// `herdr-tg adopt-secrets [--apply]`: copy every enrolled project's secret to where the channel
/// keeps one. Dry run unless `--apply`.
pub(crate) fn adopt_secrets(apply: bool) -> anyhow::Result<()> {
    adopt_in(&Registry::default_path(), apply, &mut std::io::stdout())
}

/// One row of the migration's plan.
enum Adopt {
    /// The repo holds the current secret and the channel does not: copy it, and link.
    Copy,
    /// The channel already holds the current secret: nothing to copy, link if the link is missing.
    InPlace { link_missing: bool },
    /// Nothing here can be trusted enough to copy. Said by name.
    Cannot(String),
}

fn adopt_in(
    registry_path: &Path,
    apply: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    // Refused, never guessed: a registry that is there and cannot be read is not an empty one,
    // and a migration that read `[]` off a bad byte would report a box with nothing to adopt.
    let registry = Registry::try_load(registry_path)?;
    // Held across the whole copy when writing. A rotation at another keyboard meanwhile would
    // leave a copy holding bytes the registry no longer knows — and the registry is re-read under
    // the lock so the hashes compared against are the ones on disk right now.
    let _held = if apply { Some(registry.hold()?) } else { None };
    let registry = Registry::try_load(registry_path)?;
    let home = ChannelHome::at(registry_path.parent().unwrap_or(Path::new(".")));

    let would = if apply { "" } else { "would " };
    let mut rows: Vec<&crate::registry::Project> = registry
        .all()
        .filter(|p| !crate::registry::is_room(&p.id))
        .collect();
    rows.sort_by(|a, b| a.title.cmp(&b.title));
    // A registry with nothing in it — most often a state directory that is not the hub's, from a
    // wrong `XDG_STATE_HOME` in a service or a sudo shell — is not a finished migration. Said as
    // what it is, and refused, so nobody reads "every secret is already in place" off a box
    // where nothing is anywhere.
    if rows.is_empty() {
        anyhow::bail!(
            "nothing is enrolled at {}, so there is nothing to adopt. See what is enrolled \
             with:  herdr-tg projects",
            registry_path.display()
        );
    }

    let (mut copied, mut linked, mut in_place) = (0usize, 0usize, 0usize);
    let mut cannot: Vec<String> = Vec::new();
    for p in rows {
        let current = |bytes: &str| crate::registry::sha256_hex(bytes.as_bytes()) == p.token_sha256;
        let token = p.repo.join(TOKEN_FILE);
        let repo_copy = match std::fs::read_to_string(&token) {
            Ok(s) => Some(s.trim().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                cannot.push(p.title.clone());
                writeln!(
                    out,
                    "{:<14} {} — could not read {}: {e}",
                    "cannot adopt",
                    p.title,
                    token.display()
                )?;
                continue;
            }
        };
        let channel_copy = home.read_secret(&p.id)?;
        let link_missing = home.linked(&p.repo)?.as_ref() != Some(&p.id);
        let plan = match (&repo_copy, &channel_copy) {
            (_, Some(c)) if current(c.trim()) => Adopt::InPlace { link_missing },
            (Some(r), _) if current(r) => Adopt::Copy,
            (Some(_), _) => Adopt::Cannot(format!(
                "the secret in {} is not one the hub knows, so nothing presenting it would be \
                 admitted and copying it would only spread it; enrol the project again at a \
                 terminal to mint one that is",
                token.display()
            )),
            (None, Some(_)) => Adopt::Cannot(
                "the repo holds no secret and the channel's copy is not one the hub knows; enrol \
                 the project again at a terminal"
                    .to_owned(),
            ),
            (None, None) => Adopt::Cannot(format!(
                "no secret at {}; enrol the project again at a terminal",
                token.display()
            )),
        };
        match plan {
            Adopt::Copy => {
                let bytes = repo_copy.as_deref().expect("a copy was planned from it");
                writeln!(
                    out,
                    "{:<14} {} — from {}",
                    format!("{would}copy"),
                    p.title,
                    token.display()
                )?;
                if apply {
                    home.write_secret(&p.id, bytes)?;
                    home.link_repo(&p.repo, &p.id)?;
                }
                copied += 1;
                linked += 1;
            }
            Adopt::InPlace { link_missing } => {
                writeln!(out, "{:<14} {}", "in place", p.title)?;
                in_place += 1;
                if link_missing {
                    writeln!(out, "{:<14} {} → {}", format!("{would}link"), p.title, p.id)?;
                    if apply {
                        home.link_repo(&p.repo, &p.id)?;
                    }
                    linked += 1;
                }
            }
            Adopt::Cannot(why) => {
                writeln!(out, "{:<14} {} — {why}", "cannot adopt", p.title)?;
                cannot.push(p.title.clone());
            }
        }
    }

    writeln!(out)?;
    if copied == 0 && linked == 0 {
        writeln!(
            out,
            "nothing to copy: every secret is already where the channel keeps it ({in_place} in \
             place)."
        )?;
    } else if apply {
        writeln!(
            out,
            "{copied} copied, {linked} linked, {in_place} already in place."
        )?;
        writeln!(
            out,
            "Each repo's own copy is left where it was. Take one away, per project, once you are \
             ready:  herdr-tg remove-repo-secret <repo>"
        )?;
        writeln!(out, "{}", ONLY_THIS_BUILD)?;
    } else {
        writeln!(
            out,
            "{copied} to copy, {linked} to link, {in_place} already in place. Nothing written — \
             run again with --apply to write them."
        )?;
    }
    if !cannot.is_empty() {
        anyhow::bail!(
            "{} project{} could not be adopted: {}. The others {} — see above.",
            cannot.len(),
            if cannot.len() == 1 { "" } else { "s" },
            cannot.join(", "),
            if apply { "were" } else { "would be" }
        );
    }
    Ok(())
}

/// Two homes, one writer from now on. A build from before conversations existed rewrites the
/// repo's copy alone on `enroll` and knows nothing of the channel's, so a rotation typed with it
/// after this leaves the channel's copy stale — and every new session presents the channel's
/// first, and is turned away. Said by the two verbs that make the second home matter.
const ONLY_THIS_BUILD: &str = "From now on, rotate a secret only with this build of herdr-tg — an \
    older one rewrites the repo's copy alone and leaves the channel's behind, and every new session \
    then presents the stale one. Install this build where the shell finds herdr-tg, and run the \
    hub on it.";

/// `herdr-tg remove-repo-secret <repo>`: take the repo's copy of the secret away, once the
/// channel provably holds the same bytes.
pub(crate) fn remove_repo_secret(repo: &Path) -> anyhow::Result<()> {
    remove_in(
        &Registry::default_path(),
        repo,
        std::io::stdin().is_terminal(),
        &mut std::io::stdout(),
    )
}

fn remove_in(
    registry_path: &Path,
    repo: &Path,
    at_a_terminal: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    if !at_a_terminal {
        return Err(nobody_is_typing("remove-repo-secret"));
    }
    let registry = Registry::try_load(registry_path)?;
    let home = ChannelHome::at(registry_path.parent().unwrap_or(Path::new(".")));
    let wanted = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let Some(p) = registry
        .all()
        .find(|p| p.repo == wanted && !crate::registry::is_room(&p.id))
    else {
        anyhow::bail!(
            "nothing is enrolled at {}, so there is no secret to remove. See what is enrolled \
             with:  herdr-tg projects",
            repo.display()
        );
    };
    let token = p.repo.join(TOKEN_FILE);
    let repo_copy = match std::fs::read_to_string(&token) {
        Ok(s) => s.trim().to_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            writeln!(
                out,
                "{:<14} {} holds no secret in its repo already; nothing to remove.",
                "already gone", p.title
            )?;
            return Ok(());
        }
        Err(e) => anyhow::bail!("could not read {}: {e}", token.display()),
    };

    // Every precondition this can prove, before the one thing it cannot undo.
    let Some(channel_copy) = home.read_secret(&p.id)? else {
        anyhow::bail!(
            "the channel holds no copy of {}'s secret yet, so removing the repo's would take it \
             off the air. Copy it across first:  herdr-tg adopt-secrets --apply",
            p.title
        );
    };
    let secret_path = home.secret_path(&p.id)?;
    let mode = std::fs::metadata(&secret_path)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        anyhow::bail!(
            "the channel's copy of {}'s secret is mode {mode:o}, not 600; fix that before the \
             repo's copy goes",
            p.title
        );
    }
    if channel_copy.trim() != repo_copy {
        anyhow::bail!(
            "the channel's copy of {}'s secret is not the same bytes as the repo's, so removing \
             the repo's would change what its bridge presents. Copy it across again first:  \
             herdr-tg adopt-secrets --apply",
            p.title
        );
    }
    if crate::registry::sha256_hex(repo_copy.as_bytes()) != p.token_sha256 {
        anyhow::bail!(
            "the secret in {}'s repo is not one the hub knows, so nothing is on the air with it \
             and there is nothing safe to remove. Enrol it again at a terminal.",
            p.title
        );
    }
    if home.linked(&p.repo)?.as_ref() != Some(&p.id) {
        anyhow::bail!(
            "{}'s repo is not linked to its conversation, so a session started there would find \
             nothing once the repo's copy goes. Link it first:  herdr-tg adopt-secrets --apply",
            p.title
        );
    }

    std::fs::remove_file(&token)?;
    writeln!(out, "{:<14} {}", "removed", token.display())?;
    writeln!(
        out,
        "{:<14} the channel's copy; {} is still on the air, and a session started in its folder \
         finds the secret by the repo's link.",
        "kept", p.title
    )?;
    writeln!(
        out,
        "\nWhat this cannot tell: whether a session already running has reconnected on the new \
         path. A bridge from before conversations existed reads the repo's copy on every redial, \
         so a session started before now is turned away on its next redial until it is restarted. \
         The way back is:  herdr-tg enroll {}  — a rotation, which now writes the channel's copy \
         alone; to put a repo copy back for a bridge that needs one, write the file by hand.",
        p.repo.display()
    )?;
    writeln!(out, "{ONLY_THIS_BUILD}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use std::path::PathBuf;

    use super::*;
    use crate::hub::Addr;

    /// A state directory of its own, beside the repos, so nothing a test writes lands anywhere real.
    struct Box_ {
        dir: tempfile::TempDir,
    }

    impl Box_ {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("tmp"),
            }
        }
        fn state(&self) -> PathBuf {
            self.dir.path().join("state")
        }
        fn registry(&self) -> PathBuf {
            self.state().join("projects.json")
        }
        fn home(&self) -> ChannelHome {
            ChannelHome::at(self.state())
        }
        fn repo(&self, name: &str) -> PathBuf {
            let p = self.dir.path().join(name);
            std::fs::create_dir_all(&p).expect("repo");
            p
        }
        /// Enrol the OLD way — a row and a repo token, and nothing where the channel keeps a
        /// secret — which is what every box from before this change looks like.
        fn enrol_the_old_way(&self, name: &str) -> (crate::registry::Project, String) {
            let repo = self.repo(name);
            let (p, s) = Registry::load(self.registry())
                .enrol(&repo)
                .expect("enrols");
            // The three trees go whole: a box from before this change has none of them.
            let home = self.home();
            for tree in [home.conversations(), home.by_repo(), home.grants()] {
                let _ = std::fs::remove_dir_all(tree);
            }
            (p, s)
        }
        /// Everything under the channel's three trees: path, mode, bytes. For "nothing changed".
        fn tree(&self) -> BTreeMap<PathBuf, (u32, Vec<u8>)> {
            let mut out = BTreeMap::new();
            let mut stack = vec![
                self.home().conversations(),
                self.home().by_repo(),
                self.home().grants(),
            ];
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else {
                    continue;
                };
                for e in rd.flatten() {
                    let p = e.path();
                    let meta = std::fs::symlink_metadata(&p).expect("meta");
                    if meta.is_dir() {
                        stack.push(p.clone());
                        out.insert(p, (meta.permissions().mode() & 0o777, Vec::new()));
                    } else {
                        out.insert(
                            p.clone(),
                            (
                                meta.permissions().mode() & 0o777,
                                std::fs::read(&p).expect("bytes"),
                            ),
                        );
                    }
                }
            }
            out
        }
    }

    fn said(f: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>) -> (String, Option<String>) {
        let mut out = Vec::new();
        let r = f(&mut out);
        (
            String::from_utf8(out).expect("utf8"),
            r.err().map(|e| e.to_string()),
        )
    }

    #[test]
    fn adopt_secrets_copies_every_enrolled_secret_and_writes_nothing_to_projects_json() {
        let b = Box_::new();
        let (p1, s1) = b.enrol_the_old_way("herdr-tg");
        let (p2, s2) = b.enrol_the_old_way("hub-dogfood");
        let (p3, s3) = b.enrol_the_old_way("oc-dogfood");
        let before = std::fs::read(b.registry()).expect("the registry");
        assert!(
            !b.home().conversations().exists(),
            "the fixture is not a box from before this change"
        );

        // Dry run by default: says what it would do, and writes nothing at all.
        let (plan, err) = said(|out| adopt_in(&b.registry(), false, out));
        assert!(err.is_none(), "{err:?}");
        assert!(
            !b.home().conversations().exists() && !b.home().by_repo().exists(),
            "a dry run wrote something: {plan}"
        );
        for p in [&p1, &p2, &p3] {
            assert!(
                plan.contains(&p.title),
                "the plan does not name {}: {plan}",
                p.title
            );
        }
        assert!(
            plan.contains("--apply"),
            "the plan does not say how to apply it: {plan}"
        );
        assert_eq!(std::fs::read(b.registry()).expect("the registry"), before);

        // Applied: every secret copied byte for byte, 0600, and every repo linked to its row.
        let (done, err) = said(|out| adopt_in(&b.registry(), true, out));
        assert!(err.is_none(), "{err:?}\n{done}");
        let home = b.home();
        for (p, s) in [(&p1, &s1), (&p2, &s2), (&p3, &s3)] {
            let copied = home.read_secret(&p.id).expect("reads").expect("a copy");
            assert_eq!(&copied, s, "{}'s secret was not copied whole", p.title);
            let mode = std::fs::metadata(home.secret_path(&p.id).expect("path"))
                .expect("meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
            assert_eq!(home.linked(&p.repo).expect("reads"), Some(p.id.clone()));
            // The repo's copy is left exactly where it was: that is step 7, per project, at his hand.
            assert_eq!(
                std::fs::read_to_string(p.repo.join(TOKEN_FILE)).expect("still there"),
                *s
            );
        }
        assert_eq!(
            std::fs::read(b.registry()).expect("the registry"),
            before,
            "the migration wrote projects.json"
        );
        // Every directory made is named by an id the registry already holds — nothing re-derived.
        let made: Vec<String> = std::fs::read_dir(home.conversations())
            .expect("readable")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let ids: Vec<String> = Registry::load(b.registry())
            .all()
            .map(|p| p.id.as_str().to_owned())
            .collect();
        for m in &made {
            assert!(ids.contains(m), "{m} is not an id in the registry: {ids:?}");
        }
        assert_eq!(made.len(), 3);
        for d in [b.state(), home.conversations(), home.by_repo()] {
            let mode = std::fs::metadata(&d).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{} is {mode:o}", d.display());
        }
    }

    #[test]
    fn the_migration_never_routes_through_enrol() {
        // `Registry::enrol` re-derives the id from the canonical repo path. One wrong call orphans
        // every topic a project has, permanently, carrying his history — so the migration's file
        // is not allowed to so much as spell the call. The shipped part only: a fixture below
        // enrols on purpose, to make a box from before this change.
        let src = include_str!("conversations.rs");
        let shipped = src.split("\n#[cfg(test)]").next().expect("a shipped part");
        assert!(
            !shipped.contains(".enrol("),
            "the migration file names the call that re-derives ids"
        );
    }

    #[test]
    fn adopt_secrets_on_a_box_with_nothing_enrolled_says_so_rather_than_a_finished_migration() {
        // A missing registry is an empty one for a constructor, and this walked zero rows and
        // printed "every secret is already where the channel keeps it (0 in place)" with a clean
        // exit. A wrong XDG_STATE_HOME — a service's Environment=, a sudo shell — read as "the
        // migration is done".
        let b = Box_::new();
        let (out, err) = said(|o| adopt_in(&b.registry(), false, o));
        let err = err.expect("nothing to adopt is not a finished migration");
        assert!(
            err.contains("nothing is enrolled") && err.contains("herdr-tg projects"),
            "{err}"
        );
        assert!(
            !out.contains("already where the channel keeps it") && !out.contains("in place"),
            "a box with nothing on it read as done: {out}"
        );
        assert!(
            !b.home().conversations().exists(),
            "a refusal wrote something"
        );
    }

    #[test]
    fn adopt_secrets_run_twice_changes_nothing() {
        let b = Box_::new();
        b.enrol_the_old_way("herdr-tg");
        b.enrol_the_old_way("hub-dogfood");
        let (_, err) = said(|out| adopt_in(&b.registry(), true, out));
        assert!(err.is_none(), "{err:?}");
        let once = b.tree();
        let registry = std::fs::read(b.registry()).expect("the registry");

        let (again, err) = said(|out| adopt_in(&b.registry(), true, out));
        assert!(err.is_none(), "{err:?}");
        assert_eq!(b.tree(), once, "the second run changed the tree");
        assert_eq!(std::fs::read(b.registry()).expect("the registry"), registry);
        assert!(
            again.contains("nothing to copy"),
            "the second run does not say it had nothing to do: {again}"
        );
        // And a dry run after that says the same.
        let (plan, _) = said(|out| adopt_in(&b.registry(), false, out));
        assert!(plan.contains("nothing to copy"), "{plan}");
    }

    #[test]
    fn a_migrated_project_keeps_every_topic_it_had() {
        // The asset: topic bindings hang off the id, and the id does not move. The conversation
        // directory is NAMED by the id the registry already holds, so five bindings come across
        // with no schema change and no re-derivation.
        let b = Box_::new();
        let (p, s) = b.enrol_the_old_way("hub-dogfood");
        let mut r = Registry::load(b.registry());
        r.bind_topic(&Addr::project_itself(p.id.clone()), 255)
            .expect("binds");
        for (lane, topic) in [("hub-dogfood-lane-a", 284), ("hub-dogfood-lane-b", 286)] {
            r.bind_topic(
                &Addr::lane_of(p.id.clone(), hub_proto::LaneId::new(lane)),
                topic,
            )
            .expect("binds");
        }
        let before = std::fs::read(b.registry()).expect("the registry");

        let (_, err) = said(|out| adopt_in(&b.registry(), true, out));
        assert!(err.is_none(), "{err:?}");
        assert_eq!(std::fs::read(b.registry()).expect("the registry"), before);

        let after = Registry::load(b.registry());
        let copied = b.home().read_secret(&p.id).expect("reads").expect("a copy");
        assert_eq!(copied, s);
        let resolved = after.resolve(&copied).expect("the copy resolves");
        assert_eq!(resolved.id, p.id, "the copy resolves to a different row");
        assert_eq!(resolved.topic_id, Some(255));
        assert_eq!(
            after.topic_of(&Addr::lane_of(
                p.id.clone(),
                hub_proto::LaneId::new("hub-dogfood-lane-a")
            )),
            Some(284)
        );
        assert_eq!(
            after.topic_of(&Addr::lane_of(
                p.id.clone(),
                hub_proto::LaneId::new("hub-dogfood-lane-b")
            )),
            Some(286)
        );
    }

    #[test]
    fn adopt_secrets_says_which_repo_it_cannot_adopt_and_copies_the_others() {
        // Three shapes it must not guess at: a repo whose token is gone, a repo whose token no
        // longer matches the registry (nothing that presents it would be admitted, so copying it
        // would only spread a dead secret), and a repo whose channel copy is already the current
        // one while the repo's is stale (the channel is right; leave it). Each is said by name,
        // the rest are copied, and the exit is non-zero so a script notices.
        let b = Box_::new();
        let (good, good_secret) = b.enrol_the_old_way("good");
        let (gone, _) = b.enrol_the_old_way("gone");
        std::fs::remove_file(gone.repo.join(TOKEN_FILE)).expect("rm");
        let (stale, _) = b.enrol_the_old_way("stale");
        std::fs::write(stale.repo.join(TOKEN_FILE), "0".repeat(64)).expect("write");
        let (channel_is_right, right_secret) = b.enrol_the_old_way("channel-right");
        b.home()
            .write_secret(&channel_is_right.id, &right_secret)
            .expect("the channel's copy");
        std::fs::write(channel_is_right.repo.join(TOKEN_FILE), "1".repeat(64)).expect("write");

        let (out, err) = said(|o| adopt_in(&b.registry(), true, o));
        let err = err.expect("something could not be adopted, and the exit says so");
        assert!(
            err.contains("gone") && err.contains("stale"),
            "the refusal does not name the repos it could not adopt: {err}"
        );
        assert!(out.contains(&good.title), "{out}");
        assert_eq!(
            b.home().read_secret(&good.id).expect("reads").as_deref(),
            Some(good_secret.as_str())
        );
        assert!(b.home().read_secret(&gone.id).expect("reads").is_none());
        assert!(b.home().read_secret(&stale.id).expect("reads").is_none());
        assert_eq!(
            b.home()
                .read_secret(&channel_is_right.id)
                .expect("reads")
                .as_deref(),
            Some(right_secret.as_str()),
            "a current channel copy was overwritten with a stale repo token"
        );
        assert_eq!(
            b.home().linked(&channel_is_right.repo).expect("reads"),
            Some(channel_is_right.id.clone()),
            "a project whose channel copy was already right did not get its link"
        );
        for jargon in ["Err", "None", "Some(", "sha256", "Option"] {
            assert!(
                !out.contains(jargon) && !err.contains(jargon),
                "jargon: {out}\n{err}"
            );
        }
    }

    #[test]
    fn the_book_holds_sixteen_slots_and_only_a_terminal_can_refill_it() {
        // The terminal half. The registry's own test holds the sixteen; this holds that argv is
        // not a person: an agent told to run `grant` gets a refusal and mints nothing.
        let b = Box_::new();
        let repo = b.repo("org");
        let (out, err) = said(|o| open_in(&b.registry(), &repo, false, o));
        assert!(
            err.as_deref().is_some_and(|e| e.contains("terminal")),
            "{out}\n{err:?}"
        );
        assert!(!b.registry().exists(), "an open nobody typed wrote a row");
        assert!(!b.home().conversations().exists());

        let (_, err) = said(|o| open_in(&b.registry(), &repo, true, o));
        assert!(err.is_none(), "{err:?}");
        let (out, err) = said(|o| grant_in(&b.registry(), &repo, 2, false, o));
        assert!(
            err.as_deref().is_some_and(|e| e.contains("terminal")),
            "{out}\n{err:?}"
        );
        assert_eq!(
            Registry::load(b.registry()).all().count(),
            1,
            "a grant nobody typed minted rooms"
        );

        let (out, err) = said(|o| grant_in(&b.registry(), &repo, 16, true, o));
        assert!(err.is_none(), "{err:?}\n{out}");
        assert_eq!(Registry::load(b.registry()).all().count(), 17);
        let (refused, err) = said(|o| grant_in(&b.registry(), &repo, 1, true, o));
        assert!(
            err.as_deref().is_some_and(|e| e.contains("16")),
            "{refused}\n{err:?}"
        );
        // What the operator read from the real grant names each room by its id and its slot,
        // says how full the book is, names the variable a dispatcher hands on, and echoes no
        // secret — a dispatcher needs the id and the book, and nothing else.
        let r = Registry::load(b.registry());
        let rooms: Vec<_> = r
            .all()
            .filter(|p| crate::registry::is_nothing_yet(p))
            .collect();
        assert_eq!(rooms.len(), 16);
        for room in &rooms {
            assert!(
                out.contains(room.id.as_str()),
                "a room's id is not in what he read: {out}"
            );
            let secret = b
                .home()
                .read_secret(&room.id)
                .expect("reads")
                .expect("a secret");
            assert!(!out.contains(&secret), "a secret reached stdout: {out}");
        }
        assert!(out.contains("16 of 16 vacant"), "{out}");
        assert!(out.contains("KICKOFF_HUB_CONVERSATION"), "{out}");
    }

    #[test]
    fn opening_prints_what_a_launcher_needs_and_names_no_secret() {
        let b = Box_::new();
        let repo = b.repo("herdr-tg");
        let (out, err) = said(|o| open_in(&b.registry(), &repo, true, o));
        assert!(err.is_none(), "{err:?}");
        let p = Registry::load(b.registry())
            .all()
            .next()
            .expect("a row")
            .clone();
        let secret = b
            .home()
            .read_secret(&p.id)
            .expect("reads")
            .expect("a secret");
        assert!(out.contains(p.id.as_str()), "the id is not printed: {out}");
        assert!(!out.contains(&secret), "the secret reached stdout: {out}");
        assert!(!repo.join(".kickoff").exists(), "open wrote into the repo");
        for jargon in ["Some", "None", "Option", "ProjectId"] {
            assert!(!out.contains(jargon), "jargon: {out}");
        }
        // Again: nothing minted, and it says so.
        let (again, err) = said(|o| open_in(&b.registry(), &repo, true, o));
        assert!(err.is_none(), "{err:?}");
        assert!(again.contains("already"), "{again}");
        assert_eq!(
            b.home().read_secret(&p.id).expect("reads").expect("still"),
            secret
        );
    }

    #[test]
    fn removing_a_repo_secret_is_refused_until_the_channel_holds_the_same_bytes() {
        // Step 7's verb, never step 7 itself: it runs per project, at his hand, and only once the
        // channel provably holds what the repo holds. What it cannot prove — that the bridge in
        // his live session has reconnected on the new path — it says rather than claims.
        let b = Box_::new();
        let (p, s) = b.enrol_the_old_way("herdr-tg");
        let token = p.repo.join(TOKEN_FILE);

        // Nobody at the keyboard: refused, file untouched.
        let (_, err) = said(|o| remove_in(&b.registry(), &p.repo, false, o));
        assert!(
            err.as_deref().is_some_and(|e| e.contains("terminal")),
            "{err:?}"
        );
        assert!(token.exists());

        // No channel copy yet: refused, naming the verb that makes one.
        let (_, err) = said(|o| remove_in(&b.registry(), &p.repo, true, o));
        assert!(
            err.as_deref().is_some_and(|e| e.contains("adopt-secrets")),
            "{err:?}"
        );
        assert!(
            token.exists(),
            "the repo's copy was removed with nothing to replace it"
        );

        // A channel copy that differs, LINKED: refused for the bytes. The link goes first so the
        // "not linked" refusal cannot stand in for this one — with the byte check deleted, this
        // step once still passed on that refusal alone, and proved nothing.
        b.home().link_repo(&p.repo, &p.id).expect("links");
        b.home()
            .write_secret(&p.id, "not-the-same")
            .expect("writes");
        let (_, err) = said(|o| remove_in(&b.registry(), &p.repo, true, o));
        assert!(
            err.as_deref()
                .is_some_and(|e| e.contains("not the same bytes")),
            "a differing channel copy was accepted, or refused for another reason: {err:?}"
        );
        assert!(token.exists());

        // The same bytes, linked: removed, and the caveat is said.
        b.home().write_secret(&p.id, &s).expect("writes");
        b.home().link_repo(&p.repo, &p.id).expect("links");
        let (out, err) = said(|o| remove_in(&b.registry(), &p.repo, true, o));
        assert!(err.is_none(), "{err:?}");
        assert!(!token.exists(), "the repo's copy is still there");
        assert!(
            out.contains("cannot tell") || out.contains("cannot prove"),
            "the caveat about a live session is not said: {out}"
        );
        assert!(
            out.contains("herdr-tg enroll"),
            "the way back is not named: {out}"
        );
        // The row, its hash and the channel's copy are untouched: the project is still on the air.
        let after = Registry::load(b.registry());
        assert_eq!(after.resolve(&s).expect("resolves").id, p.id);
        assert_eq!(
            b.home().read_secret(&p.id).expect("reads").as_deref(),
            Some(s.as_str())
        );

        // A second run has nothing to remove and says so, rather than failing.
        let (out, err) = said(|o| remove_in(&b.registry(), &p.repo, true, o));
        assert!(err.is_none(), "{err:?}");
        assert!(out.contains("already"), "{out}");
    }
}

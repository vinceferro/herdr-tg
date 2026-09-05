//! `herdr-tg enroll <repo>`, `disable <repo>` and `enable <repo>` — the terminal-only door.
//!
//! Admission is the one thing no message can do. Nothing arriving over Telegram or over the hub's
//! socket can add a project, mint a secret, or switch one on or off: inbound content selects from
//! what the machine already knows, and it never names something new. That boundary is only real
//! if the way in is argv, at a keyboard, which is what this file is. The listing that reads what
//! this file wrote is `projects.rs`.

use std::io::IsTerminal;
use std::path::Path;

use crate::registry::{Registry, TOKEN_FILE};

/// Enrol a repo, or rotate the secret of one already enrolled.
pub(crate) fn enrol(repo: &Path, even_if_git_would_commit_it: bool) -> anyhow::Result<()> {
    enrol_into(
        &Registry::default_path(),
        repo,
        even_if_git_would_commit_it,
        std::io::stdin().is_terminal(),
    )
}

/// The same, against a named registry and with "is a person typing this" passed in, so the door can
/// be tested without enrolling a real project into the operator's own state directory and without
/// the test needing a terminal of its own.
fn enrol_into(
    registry_path: &Path,
    repo: &Path,
    even_if_git_would_commit_it: bool,
    at_a_terminal: bool,
) -> anyhow::Result<()> {
    // ASKED FIRST, before a byte is minted or written, and that order is the whole of the guard.
    // Run afterwards — which is how it shipped — it produced a warning about a file that was
    // already on disk inside a tracked tree: a note about damage, not a door. This is the one
    // irreversible failure in this system, because a secret in a public git history cannot be
    // untracked, and the agent living in that tree commits and pushes with nobody watching.
    let exposure = secret_exposure(repo);
    if let SecretExposure::WouldCommit { said, remedy } = &exposure {
        // The override needs a PERSON, not just an argument. When a session's project is not
        // enrolled, the channel plugin hands the coding agent `Run:  herdr-tg enroll <dir>` as the
        // text of a tool result — so the likeliest reader of this refusal is an autonomous agent
        // that was just told to run this command, in a tree whose own charter has it commit and
        // push unattended. An override that a non-interactive process can pass on its own is not a
        // decision anybody made, and a refusal that prints the whole bypass command is not a
        // refusal. The flag is named in prose for that reason: a person can still find it.
        if !even_if_git_would_commit_it {
            anyhow::bail!(
                "{said}\n\nNothing has been written — no secret, and nothing enrolled. {remedy}\n\n\
                 Then run this again. If you mean to write the secret there anyway, run this at a \
                 terminal and add the --even-if-git-would-commit-it flag yourself."
            );
        }
        if !at_a_terminal {
            anyhow::bail!(
                "{said}\n\nNothing has been written — no secret, and nothing enrolled. \
                 --even-if-git-would-commit-it puts a secret inside a tree git would commit it \
                 from, and that is a thing a person has to choose: nothing here was typed at a \
                 terminal. Run it again at a keyboard if you mean it.\n\n{remedy}"
            );
        }
    }

    let mut registry = Registry::load(registry_path);
    // The secret is returned so that a caller COULD show it. This one deliberately does not: the
    // bridge reads it from the file, and echoing it here would leave a second copy in a scrollback
    // buffer, a screen recording, or a session transcript that nobody chose to put it in.
    let (project, _secret) = registry.enrol(repo)?;

    println!("enrolled       {}", project.title);
    println!("project        {}", project.id);
    println!("repo           {}", project.repo.display());
    println!("secret written {}", project.repo.join(TOKEN_FILE).display());
    match project.topic_id {
        Some(id) => println!("topic          {id} (kept from the previous enrolment)"),
        None => println!("topic          created the first time its bridge connects"),
    }

    // The secret goes to stdout exactly once, because nothing here can read it back — the registry
    // holds only a hash of it. Printing it to a pipe or a log would put it somewhere nobody chose.
    if std::io::stdout().is_terminal() {
        println!();
        println!("Its bridge reads the secret from the file above. You do not need to copy it.");
    }

    // Last, so it is the line still on the screen. `WouldCommit` only reaches here when the
    // operator asked for it by name — he is still told exactly what he has just done, because
    // meaning to do it is not the same as remembering it a week later.
    if let Some(line) = exposure.after_the_secret_is_written() {
        println!();
        println!("⚠ {line}");
    }
    Ok(())
}

/// `herdr-tg disable <repo>` and `herdr-tg enable <repo>`: the switch, at the keyboard.
///
/// Off is written into the registry, which the running hub watches — so a bridge already on the
/// socket loses its claim within about a second and is told why, and the next one to dial is
/// turned away at `hello`. What it had queued is answered `no` unsent; only a message it was
/// already in the middle of sending is finished, because a Telegram call cancelled mid-flight is
/// a message that lands with an answer saying it did not.
/// Nothing about the project is forgotten: its secret, its topic and its history all stay, and
/// `enable` is the whole of the way back.
pub(crate) fn switch(repo: &Path, on: bool) -> anyhow::Result<()> {
    let project = switch_in(&Registry::default_path(), repo, on)?;
    if on {
        println!("switched on    {}", project.title);
        println!(
            "               Its bridge is admitted the next time it dials. One that was turned \
             away tries again on its own, within a minute."
        );
    } else {
        println!("switched off   {}", project.title);
        println!(
            "               Its bridge is turned away from now on, and one already connected is \
             dropped now — a message it was in the middle of sending may still land. Its topic \
             and its history stay."
        );
        println!(
            "               Switch it back on with:  herdr-tg enable {}",
            project.repo.display()
        );
    }
    Ok(())
}

/// The same, against a named registry, so it can be tested without touching the operator's own.
fn switch_in(
    registry_path: &Path,
    repo: &Path,
    on: bool,
) -> anyhow::Result<crate::registry::Project> {
    let mut registry = Registry::load(registry_path);
    Ok(registry.set_enabled(repo, on)?)
}

/// What git says about whether this project's own secret would be committed.
///
/// Three answers and not two, because they call for three different things. "It is ignored" and
/// "there is no repository here" are both silence. "git would commit it" is the only one that
/// refuses. "I could not ask" is a warning and must never become a refusal — a machine whose git
/// is missing or broken has to be able to enrol a project.
#[derive(Debug)]
enum SecretExposure {
    /// Nothing to say: git ignores it, or there is no working tree here to commit it from.
    Silent,
    /// git says it would be committed. The whole point of the check.
    ///
    /// `remedy` is carried separately because it is not the same sentence in both shapes git can
    /// mean this in, and because it has to be said on BOTH paths — the one that refuses, and the
    /// one where the operator went ahead and the secret is now actually sitting there.
    WouldCommit { said: String, remedy: String },
    /// git would not answer. An unanswered question about a credential is said out loud.
    Unanswered(String),
}

impl SecretExposure {
    /// What is said once the secret is actually on disk, or `None` when there is nothing to say.
    fn after_the_secret_is_written(&self) -> Option<String> {
        match self {
            Self::Silent => None,
            // The remedy comes along. It used to stop at "you asked for it", which had the two
            // paths backwards: the operator who was STOPPED was told what to do, and the one now
            // holding a secret inside a tree git would commit it from was told nothing at all —
            // and this is the only path on which that secret actually reaches disk.
            Self::WouldCommit { said, remedy } => Some(format!(
                "{said} You asked for it to be written there anyway, and it is on disk now. {remedy}"
            )),
            Self::Unanswered(said) => Some(said.clone()),
        }
    }
}

/// Ask git whether this project's secret would be committed.
///
/// Asks git rather than parsing `.gitignore` by hand. The hand-rolled version read exactly one
/// file at the repo root and matched five literal strings, so it stayed SILENT for the repo shape
/// where the danger is greatest — one with no root `.gitignore` at all, where nothing is ignored
/// and everything gets committed. It also could not see a nested `.gitignore`, `core.excludesFile`,
/// or `.git/info/exclude`, all of which git honours and all of which a real repo uses.
///
/// It also used to open with `if !repo.join(".git").exists() { return None }`, which is not the
/// same question. A directory INSIDE a working tree has no `.git` of its own, so enrolling one
/// wrote a 0600 secret into a tracked tree and this guard — the only thing between that secret and
/// a public remote — never even asked git.
///
/// **Fails closed.** If git cannot answer, the operator is told rather than reassured: this is the
/// only thing standing between a secret and a public remote, and "I could not check" must never
/// render as silence.
fn secret_exposure(repo: &Path) -> SecretExposure {
    // Canonical, because the tiebreak below walks this folder's ancestors looking for a `.git`, and
    // a relative path or a symlink walks the wrong ones. A folder that will not resolve is one the
    // registry is about to refuse by name, and there is nothing to say about it here.
    let Ok(repo) = repo.canonicalize() else {
        return SecretExposure::Silent;
    };
    let repo = repo.as_path();

    let unanswered = |what: String| {
        SecretExposure::Unanswered(format!(
            "I could not ask git whether {TOKEN_FILE} is ignored in {} ({what}). Check it yourself \
             before you commit: that file is this project's secret.",
            repo.display()
        ))
    };

    match std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(o) if o.status.success() => {}
        // git ran and would not name a working tree. Either there genuinely is not one — no remote,
        // no leak, and refusing here would shut the door on every project that is not under git —
        // or git refused for its own reasons, which is an unanswered question about a credential.
        // `.git` somewhere above is what tells the two apart, and it is decided WITHOUT git, so
        // that a git that will not talk cannot vote on it.
        Ok(_) if !inside_a_working_tree(repo) => return SecretExposure::Silent,
        Ok(o) => return unanswered(format!("git exited {}", o.status)),
        Err(e) => return unanswered(e.to_string()),
    }

    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["check-ignore", "-q", TOKEN_FILE])
        .output();
    match out {
        // 0 = ignored. Nothing to say.
        Ok(o) if o.status.code() == Some(0) => SecretExposure::Silent,
        // 1 = not ignored. This is the whole point of the check — but "not ignored" covers two
        // shapes that need opposite advice, so the second question gets asked before the sentence
        // is chosen. See `already_tracked`.
        Ok(o) if o.status.code() == Some(1) && already_tracked(repo) => {
            SecretExposure::WouldCommit {
                said: format!(
                    "git already tracks {TOKEN_FILE} in {}, and that file is this project's secret.",
                    repo.display()
                ),
                remedy: format!(
                    "git is tracking that file already, so a .gitignore rule will not help — a rule \
                 only applies to files git is not tracking yet. Take it out of the index first:\n\
                 \n    git -C {} rm --cached {TOKEN_FILE}\n\nAnd if that repo has ever been \
                 pushed, treat the secret in it as public: it is in the history, and writing a new \
                 one over it does not take the old one out.",
                    repo.display()
                ),
            }
        }
        Ok(o) if o.status.code() == Some(1) => SecretExposure::WouldCommit {
            said: format!(
                "git would commit {TOKEN_FILE} in {}, and that file is this project's secret.",
                repo.display()
            ),
            remedy: format!(
                "Add {TOKEN_FILE} to that repo's .gitignore before anything in that tree is \
                 committed, or a secret written there goes wherever that repo is pushed."
            ),
        },
        // Anything else — git missing, a fatal error, a signal — is an unanswered question, and an
        // unanswered question about a credential is said out loud.
        other => unanswered(match &other {
            Ok(o) => format!("git exited {}", o.status),
            Err(e) => e.to_string(),
        }),
    }
}

/// Whether git is already TRACKING this project's secret, rather than merely willing to.
///
/// `check-ignore` consults the index, so a token file that is already tracked answers "not ignored"
/// even when a matching `.gitignore` rule is sitting right there — correctly, because git really
/// would commit a change to it. But the two shapes need opposite advice: adding the rule fixes the
/// first and does exactly nothing to the second, so an operator told to add it can follow that
/// instruction to the letter, twice, and get the identical refusal both times.
///
/// A second question git will not answer falls back to the milder wording, which is true in both
/// shapes — claiming a file is tracked when it is not would be its own piece of misinformation.
fn already_tracked(repo: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "--error-unmatch", "--", TOKEN_FILE])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Whether any folder at or above `repo` holds a `.git`.
///
/// Decided without running git, on purpose: it is the tiebreak for a `git rev-parse` that failed,
/// where "there is no repository here" and "git would not answer" mean opposite things and only
/// one of them is safe to be quiet about.
fn inside_a_working_tree(repo: &Path) -> bool {
    repo.ancestors().any(|d| d.join(".git").exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_at_the_terminal_is_written_where_the_hub_reads_it_and_the_listing_says_so() {
        // The switch is a registry write and nothing else, on purpose: the hub re-reads that file
        // on every admission and watches it for a live connection to drop, so a write here is the
        // whole of what "off" has to be. And the terminal listing has said "(switched off)" since
        // before anything could set the flag — it is worth one assertion that it still does.
        let d = tempfile::tempdir().expect("tmp");
        let registry = d.path().join("projects.json");
        let dir = d.path().join("loud-one");
        std::fs::create_dir_all(&dir).expect("dir");
        crate::registry::Registry::load(&registry)
            .enrol(&dir)
            .expect("enrols");

        let off = switch_in(&registry, &dir, false).expect("switches off");
        assert!(!off.enabled);
        let on_disk = crate::registry::Registry::load(&registry);
        assert!(
            !on_disk.get(&off.id).expect("still enrolled").enabled,
            "the switch was not written where the hub reads it"
        );

        let on = switch_in(&registry, &dir, true).expect("switches on");
        assert!(on.enabled);
        assert!(
            crate::registry::Registry::load(&registry)
                .get(&on.id)
                .expect("still enrolled")
                .enabled
        );

        // A path nobody enrolled is a refusal in plain words, never an enrolment.
        let said = switch_in(&registry, &d.path().join("nobody"), true)
            .expect_err("refused")
            .to_string();
        assert!(said.contains("nothing is enrolled"), "{said}");
        assert_eq!(
            crate::registry::Registry::load(&registry).all().count(),
            1,
            "a switch enrolled something"
        );
    }

    /// A real git repo, because the check now asks git and a fake `.git` directory is not one.
    fn repo_with(gitignore: Option<&str>) -> tempfile::TempDir {
        let d = tempfile::tempdir().expect("tmp");
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(d.path())
            .args(["init", "-q"])
            .status()
            .expect("run git");
        assert!(ok.success(), "git init failed");
        if let Some(body) = gitignore {
            std::fs::write(d.path().join(".gitignore"), body).expect("write");
        }
        d
    }

    /// A folder that is genuinely outside any git working tree.
    ///
    /// Stated rather than assumed. `tempfile::tempdir()` puts its folder wherever TMPDIR points,
    /// this repo tells every agent to choose TMPDIR by hand, and a short absolute path inside a
    /// checkout is an obvious pick — at which point the fixture IS in a repo, the door correctly
    /// refuses it, and two tests go red as though the door were broken. One self-explaining line
    /// beats that.
    fn folder_outside_any_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().expect("tmp");
        let canonical = d.path().canonicalize().expect("canonical");
        assert!(
            !inside_a_working_tree(&canonical),
            "TMPDIR points inside a git working tree, so this test's fixture is itself in a repo \
             and the door is right to refuse it. Point TMPDIR at somewhere outside a checkout. \
             The fixture landed at {}",
            canonical.display()
        );
        d
    }

    /// A repo that has actually COMMITTED its own secret, with a .gitignore rule for it as well.
    ///
    /// This is the post-leak shape, and the one where the two facts come apart: git honours the
    /// rule for new files and goes on tracking this one regardless, so `check-ignore` correctly
    /// answers "not ignored" and the remedy is not the rule.
    fn repo_that_already_committed_the_secret() -> tempfile::TempDir {
        let d = repo_with(Some(&format!("{TOKEN_FILE}\n")));
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(args)
                .output()
                .expect("run git");
            assert!(ok.status.success(), "git {args:?} failed");
        };
        git(&["config", "user.email", "nobody@example.invalid"]);
        git(&["config", "user.name", "a test"]);
        std::fs::create_dir_all(d.path().join(".kickoff")).expect("dir");
        std::fs::write(d.path().join(TOKEN_FILE), "an-old-secret\n").expect("write");
        git(&["add", "-f", ".gitignore", TOKEN_FILE]);
        git(&["commit", "-qm", "the shape this is about"]);
        d
    }

    #[test]
    fn the_override_is_refused_unless_a_person_is_at_the_keyboard() {
        // The flag exists so the operator can say he means it. As first built it was honoured with
        // stdin, stdout and stderr all closed, from a non-interactive shell — and the refusal ended
        // with a complete, ready-to-paste command line containing the flag.
        //
        // That combination matters here and not in a generic tool. When a session's project is not
        // enrolled, the channel plugin returns `Run:  herdr-tg enroll <dir>` as the text of a tool
        // result, so the reader of this refusal is most often an autonomous agent that was just
        // told to run this command, inside a tree whose own charter has it commit and push
        // unattended. A refusal that hands that reader the bypass is not a refusal.
        let d = repo_with(Some("target/\n"));
        let state = tempfile::tempdir().expect("tmp");
        let registry = state.path().join("projects.json");

        let refused = enrol_into(&registry, d.path(), true, false)
            .expect_err("the override must not be honoured with nobody at the keyboard");
        assert!(
            !d.path().join(TOKEN_FILE).exists() && !registry.exists(),
            "a secret was written for an override nobody typed"
        );
        assert!(
            refused.to_string().contains("terminal"),
            "it does not say what is missing: {refused}"
        );

        // And the ordinary refusal stops being a copy-paste. It still NAMES the flag — a door with
        // no handle is a door nobody can use — but naming it and handing over the whole command
        // line are different things when the reader is a machine.
        let plain = enrol_into(&registry, d.path(), false, false).expect_err("it must refuse");
        assert!(
            plain.to_string().contains("--even-if-git-would-commit-it"),
            "it refuses without saying how to go ahead: {plain}"
        );
        assert!(
            !plain.to_string().contains("herdr-tg enroll"),
            "the refusal hands the reader the exact command that bypasses it: {plain}"
        );

        // Typed at a keyboard it still goes through, because this is also the rotation path.
        enrol_into(&registry, d.path(), true, true).expect("a person may still say he means it");
        assert!(d.path().join(TOKEN_FILE).exists());
    }

    #[test]
    fn a_secret_git_already_tracks_is_told_the_one_remedy_that_untracks_it() {
        // `check-ignore` consults the index, so a token file that is ALREADY TRACKED answers "not
        // ignored" even with a matching rule in .gitignore — correctly, because git really would
        // commit a change to it. The refusal then said "add it to that repo's .gitignore and run
        // this again", which in this shape is advice the operator can follow to the letter, twice,
        // and get the identical refusal both times. The one thing git needs was never named.
        //
        // This is the post-leak shape, which is exactly when rotating a secret matters most.
        let d = repo_that_already_committed_the_secret();
        let (said, remedy) = would_commit(d.path());
        assert!(
            remedy.contains("rm --cached"),
            "the only remedy that clears this is never named: {said} / {remedy}"
        );
        assert!(
            !remedy.contains(".gitignore") || remedy.contains("will not"),
            "it still prescribes the rule that is already there and changes nothing: {remedy}"
        );
        // The secret is in the history, and a fresh one written over it does not take the old one
        // out. He is owed that, because it is the difference between rotating and being safe.
        assert!(
            remedy.contains("history"),
            "it does not say the old secret is already in the history: {remedy}"
        );
    }

    #[test]
    fn going_ahead_anyway_still_says_what_to_do_about_the_secret_now_sitting_there() {
        // Backwards, as first built: the operator who was STOPPED was told what to do, and the one
        // holding a live exposure was told only "You asked for it to be written there anyway." The
        // flag path is the only path on which the secret actually reaches disk inside a tree git
        // would commit it from, so it is the path where the remedy is load-bearing.
        let untracked = repo_with(Some("target/\n"));
        let line = secret_exposure(untracked.path())
            .after_the_secret_is_written()
            .expect("it must still say something");
        assert!(
            line.contains(".gitignore"),
            "the operator is left holding a live exposure with no idea what to do: {line}"
        );

        let tracked = repo_that_already_committed_the_secret();
        let line = secret_exposure(tracked.path())
            .after_the_secret_is_written()
            .expect("it must still say something");
        assert!(
            line.contains("rm --cached"),
            "a secret written over one git already tracks, with no way out named: {line}"
        );
    }

    #[test]
    fn a_repo_whose_git_would_commit_the_secret_is_refused_before_the_secret_exists() {
        // The one irreversible failure this system has. A secret in a public git history cannot be
        // untracked, and the coordinator living in that tree commits and pushes with nobody
        // watching. The check existed and it ran AFTER the mint, so what it produced was not a
        // guard but a note about damage already done: the file was on disk, inside a tracked tree,
        // before the operator read a word about it.
        let d = repo_with(Some("target/\n"));
        let state = tempfile::tempdir().expect("tmp");
        let registry = state.path().join("projects.json");

        let refused = enrol_into(&registry, d.path(), false, true).expect_err("it must refuse");
        let said = refused.to_string();
        assert!(
            said.contains(TOKEN_FILE),
            "it does not name the file: {said}"
        );

        assert!(
            !d.path().join(TOKEN_FILE).exists(),
            "the secret was written into a tree git would commit it from"
        );
        assert!(
            !registry.exists(),
            "the project was enrolled even though its secret was refused"
        );

        // And it says how to mean it anyway, because `enroll` is also how a leaked secret is
        // rotated: a door with no handle is a door nobody can use.
        assert!(
            said.contains("--even-if-git-would-commit-it"),
            "it refuses without saying how to go ahead: {said}"
        );

        // Said out loud, it goes through — and only then is anything written.
        enrol_into(&registry, d.path(), true, true)
            .expect("a deliberate rotation is still possible");
        assert!(d.path().join(TOKEN_FILE).exists());
        assert!(registry.exists());
    }

    #[test]
    fn a_folder_that_is_not_a_git_repo_at_all_still_enrols() {
        // Getting this wrong shuts the door on the operator. There is no repository, so there is no
        // remote and nothing to leak — and a refusal here would fire on every project that is not
        // under git, for a danger that does not exist.
        let d = folder_outside_any_repo();
        let state = tempfile::tempdir().expect("tmp");
        let registry = state.path().join("projects.json");
        enrol_into(&registry, d.path(), false, true)
            .expect("a folder with no git must still enrol");
        assert!(d.path().join(TOKEN_FILE).exists());
    }

    #[test]
    fn a_repo_git_will_not_answer_about_is_warned_about_rather_than_refused() {
        // "I could not check" and "I checked, and it would be committed" are different answers, and
        // only one of them is a refusal. A machine whose git is broken, missing, or looking at a
        // `.git` it cannot read must still be able to enrol a project.
        let d = tempfile::tempdir().expect("tmp");
        // A `.git` that is a file and not a gitdir: git refuses to name a working tree, while the
        // tiebreak — decided without git, so a git that will not talk cannot vote on it — still
        // sees that this is inside one.
        std::fs::write(d.path().join(".git"), "gitdir: nowhere-that-exists\n").expect("write");
        assert!(matches!(
            secret_exposure(d.path()),
            SecretExposure::Unanswered(_)
        ));

        let state = tempfile::tempdir().expect("tmp");
        let registry = state.path().join("projects.json");
        enrol_into(&registry, d.path(), false, true)
            .expect("an unanswered question is not a refusal");
        assert!(d.path().join(TOKEN_FILE).exists());
    }

    /// The one answer that refuses, told apart from the two that do not.
    fn would_commit(d: &Path) -> (String, String) {
        match secret_exposure(d) {
            SecretExposure::WouldCommit { said, remedy } => (said, remedy),
            other => panic!("git was expected to say it would commit the secret, got {other:?}"),
        }
    }

    #[test]
    fn a_repo_that_would_commit_its_own_secret_is_told_apart_from_one_that_would_not() {
        let d = repo_with(Some("target/\n"));
        assert!(would_commit(d.path()).0.contains(TOKEN_FILE));
    }

    #[test]
    fn a_repo_with_no_gitignore_at_all_is_the_shape_git_says_would_commit_it() {
        // The shape the hand-rolled check stayed silent for, and the one where the danger is
        // greatest: nothing is ignored, so everything gets committed.
        let d = repo_with(None);
        assert!(would_commit(d.path()).0.contains(TOKEN_FILE));
    }

    #[test]
    fn a_repo_that_already_ignores_the_secret_is_left_alone() {
        for line in [".kickoff/", ".kickoff", ".kickoff/hub.token", "*.token"] {
            let d = repo_with(Some(&format!("target/\n{line}\n")));
            assert!(
                matches!(secret_exposure(d.path()), SecretExposure::Silent),
                "a repo already ignoring {line} was not let through"
            );
        }
    }

    #[test]
    fn a_nested_gitignore_counts_because_git_says_it_does() {
        // The hand-rolled version read only the root file. git reads every level, and so a project
        // that ignores its secret one directory down was warned about for nothing — which is how a
        // warning gets trained away before the one that matters.
        let d = repo_with(Some("target/\n"));
        std::fs::create_dir_all(d.path().join(".kickoff")).expect("dir");
        std::fs::write(d.path().join(".kickoff/.gitignore"), "hub.token\n").expect("write");
        assert!(matches!(secret_exposure(d.path()), SecretExposure::Silent));
    }

    #[test]
    fn a_folder_inside_a_working_tree_is_checked_rather_than_waved_through() {
        // It has no `.git` of its own, which is exactly why the guard used to return before asking
        // git anything at all — and a secret written there is a secret in a tracked tree.
        let d = repo_with(Some(".kickoff/hub.token\n"));
        let inside = d.path().join("crates");
        std::fs::create_dir_all(&inside).expect("dir");
        assert!(would_commit(&inside).0.contains(TOKEN_FILE));
    }

    #[test]
    fn a_directory_that_is_not_a_git_repo_is_let_through() {
        // No remote, no leak. A warning here would fire on every non-git project and teach the
        // operator to ignore the one that counts.
        let d = folder_outside_any_repo();
        assert!(matches!(secret_exposure(d.path()), SecretExposure::Silent));
    }
}

//! `herdr-tg enroll <repo>` and `herdr-tg projects` — the terminal-only door.
//!
//! Admission is the one thing no message can do. Nothing arriving over Telegram or over the hub's
//! socket can add a project, mint a secret, or switch one on: inbound content selects from what the
//! machine already knows, and it never names something new. That boundary is only real if the way
//! in is argv, at a keyboard, which is what this file is.

use std::io::IsTerminal;
use std::path::Path;

use crate::registry::{Registry, TOKEN_FILE};

/// Enrol a repo, or rotate the secret of one already enrolled.
pub(crate) fn enrol(repo: &Path) -> anyhow::Result<()> {
    let path = Registry::default_path();
    let mut registry = Registry::load(&path);
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

    // A secret in a repo whose remote is public is a secret on the internet. This repo's own
    // guardrail says so in as many words, and the check is three lines, so it is checked rather
    // than left to a habit.
    if let Some(warning) = gitignore_gap(&project.repo) {
        println!();
        println!("⚠ {warning}");
    }
    Ok(())
}

/// Every enrolled project, for a human at the keyboard.
pub(crate) fn projects() -> anyhow::Result<()> {
    let registry = Registry::load(Registry::default_path());
    let mut any = false;
    for p in registry.all() {
        any = true;
        let topic = match p.topic_id {
            Some(id) => format!("topic {id}"),
            None => "not connected yet".to_owned(),
        };
        let state = if p.enabled { "" } else { "  (switched off)" };
        println!(
            "{:<24} {:<18} {}{}",
            p.title,
            topic,
            p.repo.display(),
            state
        );
    }
    if !any {
        println!("Nothing is enrolled yet. Add a project with:  herdr-tg enroll <repo>");
    }
    Ok(())
}

/// `Some(sentence)` when the project's own secret could be committed.
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
/// **Fails closed.** If git cannot answer, the operator is warned rather than reassured: this is
/// the only thing standing between a bot token and a public remote, and "I could not check" must
/// never render as silence.
fn gitignore_gap(repo: &Path) -> Option<String> {
    let unanswered = |what: String| {
        Some(format!(
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
        // no leak, and a warning here would fire on every non-git project and teach the operator to
        // ignore the one that counts — or git refused for its own reasons, which is an unanswered
        // question about a credential. `.git` somewhere above is what tells the two apart, and it
        // is decided WITHOUT git, so that a git that will not talk cannot vote on it.
        Ok(_) if !inside_a_working_tree(repo) => return None,
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
        Ok(o) if o.status.code() == Some(0) => None,
        // 1 = not ignored. This is the whole point of the check.
        Ok(o) if o.status.code() == Some(1) => Some(format!(
            "git would commit {TOKEN_FILE} in {}, and that file is this project's secret. \
             If this repo has a public remote, add the line to .gitignore before you commit \
             anything.",
            repo.display()
        )),
        // Anything else — git missing, a fatal error, a signal — is an unanswered question, and an
        // unanswered question about a credential is a warning.
        other => unanswered(match &other {
            Ok(o) => format!("git exited {}", o.status),
            Err(e) => e.to_string(),
        }),
    }
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

    #[test]
    fn a_repo_that_would_commit_its_own_secret_is_warned_about() {
        let d = repo_with(Some("target/\n"));
        let said = gitignore_gap(d.path()).expect("a warning");
        assert!(said.contains(TOKEN_FILE), "{said}");
    }

    #[test]
    fn a_repo_with_no_gitignore_at_all_is_warned_about() {
        // The shape the hand-rolled check stayed silent for, and the one where the danger is
        // greatest: nothing is ignored, so everything gets committed.
        let d = repo_with(None);
        let said = gitignore_gap(d.path()).expect("a warning");
        assert!(said.contains(TOKEN_FILE), "{said}");
    }

    #[test]
    fn a_repo_that_already_ignores_the_secret_is_left_alone() {
        for line in [".kickoff/", ".kickoff", ".kickoff/hub.token", "*.token"] {
            let d = repo_with(Some(&format!("target/\n{line}\n")));
            assert!(
                gitignore_gap(d.path()).is_none(),
                "warned about a repo already ignoring {line}"
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
        assert!(gitignore_gap(d.path()).is_none());
    }

    #[test]
    fn a_folder_inside_a_working_tree_is_checked_rather_than_waved_through() {
        // It has no `.git` of its own, which is exactly why the guard used to return before asking
        // git anything at all — and a secret written there is a secret in a tracked tree.
        let d = repo_with(Some(".kickoff/hub.token\n"));
        let inside = d.path().join("crates");
        std::fs::create_dir_all(&inside).expect("dir");
        let said = gitignore_gap(&inside).expect("a warning");
        assert!(said.contains(TOKEN_FILE), "{said}");
    }

    #[test]
    fn a_directory_that_is_not_a_git_repo_is_not_warned_about() {
        // No remote, no leak. A warning here would fire on every non-git project and teach the
        // operator to ignore the one that counts.
        let d = tempfile::tempdir().expect("tmp");
        assert!(gitignore_gap(d.path()).is_none());
    }
}

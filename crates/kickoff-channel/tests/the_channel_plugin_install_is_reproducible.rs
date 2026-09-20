//! **The bridge on the box is the bridge in this repo, or the install fails.**
//!
//! Measured on a real box on 8 September 2026, and this is the defect these tests exist for: the
//! cache directory the marketplace installed into held a `server.ts` of 15262 bytes dated
//! 1 September with no `hub-link.ts` beside it at all, while the repo's copy was 74665 bytes with
//! `hub-link.ts` and three more files. Both declared version `0.1.0`. The cache directory is keyed
//! by the version, the installer only ever ran the install verb, and an install of a version that
//! is already there is a no-op — so on every box that already had the plugin, a week of fixes was
//! never picked up and nothing said so. The bridge in that cache stamps no lease and promises no
//! confirmation, so a tap on the phone reads as sent for ever and the fence that is supposed to
//! catch a replaced run never engages.
//!
//! Two things have to be true for that to stop happening, and this file pins both.
//!
//! # 1. The version moves with the content
//!
//! The version IS the cache key, so only a version that changes when the bytes change can bust the
//! cache. We chose a **hand-bumped version with a gate** over a version derived from a content
//! hash, and the reason is that the version is not only a cache key: `claude plugin list` shows it
//! to a person, `installed_plugins.json` records it, and the installer compares it against what is
//! on the box to decide whether the copy there is OLDER than this one. A hash answers "different",
//! never "older", so the update path would have nothing to stand on; and a hash written into two
//! tracked manifests would be rewritten by every edit, which is a second thing to keep current for
//! no gain. So a human bumps the number in THREE files — the two manifests and the server that
//! announces itself to its client — and `the_channel_plugins_version_moves_with_its_content`
//! refuses to let that be forgotten. It compares the plugin's tracked bytes now against **every**
//! commit that carries this version, not only the newest run of them: a number that shipped, was
//! superseded and was then reverted back to is the same silent no-op reached by `git revert`, and a
//! walk that stops at the first differing version cannot see it.
//!
//! # 2. The installer fails loud, and verifies afterwards
//!
//! The rest of the file drives `scripts/install-channel-plugin.sh` against **fixtures**: a fake
//! `HOME`, a fake repository, a fake managed-settings file, and `claude`, `bun` and `cargo` on
//! `PATH` that record every call they are given and misbehave the way the mode asks. It never runs
//! the real installer, never reads or writes the operator's `~/.claude`, and the fake `claude`
//! exits 2 on sight if `HOME` is the invoking user's real home — so a fixture that leaks turns into
//! a failed test rather than a modified box.
//!
//! Every refusal the installer can reach has a test that drives it, because a guard with no test is
//! a guard the next rewrite deletes without noticing: a swallowed exit code, a registry shape it has
//! never read, a copy stored under the wrong directory, a marketplace pointing elsewhere, a file on
//! the box the repo cannot account for, a door read as open when it is shut.
//!
//! ## This guard FAILS when it cannot look
//!
//! There is no path here from "found nothing" to green. A missing tool, a repository the gate
//! cannot read, a `git log` that returns no commit for a path that is plainly tracked — each is a
//! failure, never an empty comparison that passes.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// ───────────────────────────── shared plumbing ─────────────────────────────

/// The workspace root, from this crate's manifest. Fails closed rather than guessing from the cwd,
/// which differs between `cargo test` and a hand-run binary.
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("no workspace root above {}", manifest.display()))
        .to_path_buf();
    assert!(
        root.join("Cargo.toml").is_file(),
        "expected a workspace manifest at {}",
        root.join("Cargo.toml").display()
    );
    root
}

/// Refuse to run at all when a tool the fixtures need is absent — a selftest that quietly skips is
/// how a broken installer ships.
fn require_tool(name: &str) {
    let found = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .unwrap_or_else(|e| panic!("could not look for {name}: {e}"));
    assert!(
        found.status.success(),
        "this guard needs `{name}` on PATH and cannot prove anything without it"
    );
}

/// Run git in `dir`, failing the test on anything but a clean exit.
fn git(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap_or_else(|e| panic!("could not start git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {:?} in {} failed ({}): {}",
        args,
        dir.display(),
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn git_text(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(git(dir, args)).expect("git printed something that is not text")
}

/// The `"version"` value out of a JSON manifest, without pulling a parser into a guard whose whole
/// job is to be simpler than what it guards.
fn version_in(json: &str, what: &str) -> String {
    let key = "\"version\"";
    let at = json
        .find(key)
        .unwrap_or_else(|| panic!("{what} has no version field"));
    let rest = &json[at + key.len()..];
    let colon = rest
        .find(':')
        .unwrap_or_else(|| panic!("{what}'s version field has no value"));
    let rest = &rest[colon + 1..];
    let open = rest
        .find('"')
        .unwrap_or_else(|| panic!("{what}'s version is not a string"));
    let rest = &rest[open + 1..];
    let close = rest
        .find('"')
        .unwrap_or_else(|| panic!("{what}'s version string never ends"));
    rest[..close].to_string()
}

// ───────────────────── 1. the version moves with the content ─────────────────────

const PLUGIN_DIR: &str = "plugins/kickoff-channel";

#[test]
fn the_channel_plugins_version_moves_with_its_content() {
    require_tool("git");
    if let Err(complaint) = version_drift(&repo_root()) {
        panic!("{complaint}");
    }
}

/// The gate, over any checkout, so the proof that it BITES can be run against a repository built
/// for the purpose rather than only against this one. A gate that has only ever been watched
/// passing is a gate nobody has seen work.
///
/// `Err` is the complaint, written for whoever is about to be stopped by it.
fn version_drift(root: &Path) -> Result<(), String> {
    let plugin = root.join(PLUGIN_DIR);
    let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json"))
        .expect("the plugin manifest must be readable");
    let package = fs::read_to_string(plugin.join("package.json"))
        .expect("the package manifest must be readable");
    let current = version_in(&manifest, "the plugin manifest");
    let package_version = version_in(&package, "the plugin's package manifest");

    // One version, spelled twice: the manifest is what the marketplace keys the cache by, the
    // package file is what bun and every reader of the directory sees. They drift silently.
    if current != package_version {
        return Err(format!(
            "the plugin manifest says version {current} and package.json says {package_version}; \
             they are one version and must be bumped together"
        ));
    }
    if current.trim().is_empty() {
        return Err("the plugin has no version, and the version is what busts the cache".into());
    }

    // And a third time, in the server the bridge actually runs. No manifest reader ever looks at
    // it, which is how it drifted a week behind the other two the one time a bump left it out: the
    // box then holds one number and the running server tells its client another.
    if let Some(announced) = announced_version(&plugin) {
        if announced != current {
            return Err(format!(
                "the manifests say version {current} and {PLUGIN_DIR}/server.ts announces \
                 {announced} to its client; they are one version and must be bumped together"
            ));
        }
    }

    // EVERY commit that carries this version, not only the newest run of them. Stopping at the
    // first commit whose version differs only ever proves "these bytes have not moved since this
    // number was last set" — so a number that was published, superseded, and then reverted back to
    // walks back no further than the revert, while every box that holds the FIRST copy of that
    // number keeps it for ever and nothing says so. That is the same silent no-op this whole file
    // exists to stop, reached by `git revert` rather than by forgetting.
    let log = git_text(root, &["log", "--format=%H", "--", PLUGIN_DIR]);
    let commits: Vec<&str> = log.split_whitespace().collect();
    assert!(
        !commits.is_empty(),
        "no commit in {} has ever touched {PLUGIN_DIR}, which cannot be true — this guard cannot \
         compare anything and will not pass by default",
        root.display()
    );

    let now = tracked_files_now(root);

    // Oldest first, so the complaint names the first commit that published these bytes under this
    // number — the copy that is out on boxes, not the most recent one.
    for sha in commits.iter().rev() {
        let Some(manifest_then) = manifest_at(root, sha) else {
            continue; // the manifest did not exist at this commit: nothing carried a version
        };
        if version_in(&manifest_then, "a past plugin manifest") != current {
            continue;
        }
        let then = tracked_files_at(root, sha);

        let mut changed: Vec<String> = Vec::new();
        for (path, bytes) in &now {
            match then.get(path) {
                None => changed.push(format!("added   {path}")),
                Some(before) if before != bytes => changed.push(format!("changed {path}")),
                Some(_) => {}
            }
        }
        for path in then.keys() {
            if !now.contains_key(path) {
                changed.push(format!("removed {path}"));
            }
        }
        if changed.is_empty() {
            continue;
        }
        return Err(format!(
            "the plugin's files changed but its version did not.\n\
             Version {current} was published in {short}, and the tree now differs from it:\n  \
             {list}\n\
             The version is the cache key: a box that already has {current} installs nothing and \
             keeps the old bridge, silently. Bump the version in both \
             {PLUGIN_DIR}/.claude-plugin/plugin.json and {PLUGIN_DIR}/package.json in the same \
             commit as the change, and never re-use a number that has already shipped.",
            short = &sha[..12],
            list = changed.join("\n  "),
        ));
    }
    Ok(())
}

/// The plugin manifest as it stood at `sha`, or `None` when the plugin did not have one there.
fn manifest_at(root: &Path, sha: &str) -> Option<String> {
    let path = format!("{sha}:{PLUGIN_DIR}/.claude-plugin/plugin.json");
    let shown = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &path])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("could not ask git for a past manifest");
    shown
        .status
        .success()
        .then(|| String::from_utf8_lossy(&shown.stdout).into_owned())
}

/// The version the MCP server announces to whatever started it, if it spells one out at all.
///
/// This is the third place the number is written and the only one no installer, marketplace or
/// registry ever reads — which is exactly why it is the one that gets left behind.
fn announced_version(plugin: &Path) -> Option<String> {
    let source = fs::read_to_string(plugin.join("server.ts")).ok()?;
    let at = source.find("name: 'kickoff-channel'")?;
    let window = &source[at..source.len().min(at + 200)];
    let key = window.find("version: '")?;
    let tail = &window[key + "version: '".len()..];
    let end = tail.find('\'')?;
    Some(tail[..end].to_string())
}

#[test]
fn the_bridge_announces_the_version_its_manifests_carry() {
    let plugin = repo_root().join(PLUGIN_DIR);
    let announced = announced_version(&plugin).expect(
        "server.ts no longer spells its version out where this guard can see it. It is the copy of \
         the number nothing else reads, so if it has moved, move this reader with it rather than \
         letting the check quietly stop looking.",
    );
    let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json"))
        .expect("the plugin manifest must be readable");
    assert_eq!(
        announced,
        version_in(&manifest, "the plugin manifest"),
        "the bridge announces one version to its client and the box has another"
    );
}

#[test]
fn a_version_reused_after_a_bump_carries_different_bytes_and_is_refused() {
    require_tool("git");
    let checkout = Checkout::new();
    checkout.write("0.1.0", "// the bridge every box already has\n");
    checkout.first_commit("0.1.0 ships");
    checkout.write("0.2.0", "// a bridge that was tried\n");
    checkout.commit("0.2.0 ships");

    // A `git revert` of the bump: the number goes back to one boxes already hold, and the bytes
    // wearing it are not the bytes they hold.
    checkout.write(
        "0.1.0",
        "// a DIFFERENT bridge, wearing a version this box already has\n",
    );
    checkout.commit("back to 0.1.0");

    let complaint = version_drift(&checkout.root).expect_err(
        "0.1.0 is on boxes already; re-using the number with other bytes is exactly the silent \
         no-op this gate exists to stop",
    );
    assert!(
        complaint.contains("server.ts") && complaint.contains("0.1.0"),
        "the complaint must name the file that moved and the number it moved under: {complaint}"
    );
}

#[test]
fn a_version_reverted_to_exactly_the_bytes_that_shipped_under_it_is_not_refused() {
    // The other half of the same rule, so the gate is not simply "a version may never repeat": a
    // revert that puts back exactly what that number always meant leaves every box correct.
    require_tool("git");
    let checkout = Checkout::new();
    checkout.write("0.1.0", "// the bridge every box already has\n");
    checkout.first_commit("0.1.0 ships");
    checkout.write("0.2.0", "// a bridge that was tried\n");
    checkout.commit("0.2.0 ships");
    checkout.write("0.1.0", "// the bridge every box already has\n");
    checkout.commit("back to 0.1.0, unchanged");
    assert!(
        version_drift(&checkout.root).is_ok(),
        "a version that means what it has always meant is not drift"
    );
}

/// A throwaway checkout holding nothing but the plugin, for driving the version gate over
/// histories this repository does not have.
struct Checkout {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Checkout {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("no temporary directory");
        let root = dir.path().join("checkout");
        fs::create_dir_all(root.join(PLUGIN_DIR).join(".claude-plugin")).unwrap();
        Checkout { _dir: dir, root }
    }

    fn write(&self, version: &str, body: &str) {
        let plugin = self.root.join(PLUGIN_DIR);
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!("{{ \"version\": \"{version}\" }}\n"),
        )
        .unwrap();
        fs::write(
            plugin.join("package.json"),
            format!("{{ \"version\": \"{version}\" }}\n"),
        )
        .unwrap();
        fs::write(plugin.join("server.ts"), body).unwrap();
    }

    fn first_commit(&self, message: &str) {
        git(&self.root, &["init", "-q", "-b", "main"]);
        self.commit(message);
    }

    fn commit(&self, message: &str) {
        git(&self.root, &["add", "-A"]);
        git(
            &self.root,
            &[
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "user.name=fixture",
                "commit",
                "-q",
                "-m",
                message,
                "--no-verify",
            ],
        );
    }
}

#[test]
fn the_version_gate_catches_a_change_committed_without_a_bump_and_lets_the_bump_through() {
    require_tool("git");
    let dir = tempfile::tempdir().expect("no temporary directory");
    let root = dir.path().join("checkout");
    let plugin = root.join(PLUGIN_DIR);
    fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();

    let write = |version: &str, body: &str| {
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!("{{ \"version\": \"{version}\" }}\n"),
        )
        .unwrap();
        fs::write(
            plugin.join("package.json"),
            format!("{{ \"version\": \"{version}\" }}\n"),
        )
        .unwrap();
        fs::write(plugin.join("server.ts"), body).unwrap();
    };
    let commit = |message: &str| {
        git(&root, &["add", "-A"]);
        git(
            &root,
            &[
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "user.name=fixture",
                "commit",
                "-q",
                "-m",
                message,
                "--no-verify",
            ],
        );
    };

    write("0.1.0", "// the first bridge\n");
    git(&root, &["init", "-q", "-b", "main"]);
    commit("the plugin arrives");
    assert!(
        version_drift(&root).is_ok(),
        "a tree that matches the commit its version was set in is not drifting"
    );

    // The defect, reproduced: the bytes change, the version does not, and the commit lands.
    write("0.1.0", "// this week's bridge\n");
    commit("a fix nobody bumped for");
    let complaint = version_drift(&root).expect_err(
        "a plugin whose files changed under an unchanged version must be refused — that is the          whole reason a week-old bridge stayed on a box for a week",
    );
    assert!(
        complaint.contains("server.ts") && complaint.contains("0.1.0"),
        "the complaint must name the file and the version that did not move: {complaint}"
    );

    // A bump in the working tree is the fix, and it is accepted before it is committed.
    write("0.2.0", "// this week's bridge\n");
    assert!(
        version_drift(&root).is_ok(),
        "a bump that has not been committed yet is a person doing the right thing"
    );

    // And once the bump IS committed, the gate goes back to watching from there.
    commit("the bump");
    assert!(
        version_drift(&root).is_ok(),
        "the bumped tree matches its own commit"
    );
    write("0.2.0", "// next week's bridge\n");
    commit("another fix nobody bumped for");
    assert!(
        version_drift(&root).is_err(),
        "the gate must keep biting after a bump, not only before the first one"
    );
}

/// The plugin's tracked files at a commit, keyed by their path below the plugin directory.
fn tracked_files_at(root: &Path, sha: &str) -> BTreeMap<String, Vec<u8>> {
    let listing = git_text(
        root,
        &["ls-tree", "-r", "--name-only", sha, "--", PLUGIN_DIR],
    );
    let mut out = BTreeMap::new();
    for path in listing.lines().map(str::trim).filter(|p| !p.is_empty()) {
        let bytes = git(root, &["show", &format!("{sha}:{path}")]);
        out.insert(relative_to_plugin(path), bytes);
    }
    assert!(
        !out.is_empty(),
        "the plugin directory is empty at {sha}, which this guard cannot compare against"
    );
    out
}

/// The plugin's tracked files as they are on disk right now — the bytes an install would copy.
fn tracked_files_now(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let listing = git_text(root, &["ls-files", "--", PLUGIN_DIR]);
    let mut out = BTreeMap::new();
    for path in listing.lines().map(str::trim).filter(|p| !p.is_empty()) {
        let bytes = fs::read(root.join(path)).unwrap_or_else(|e| {
            panic!("{path} is tracked but cannot be read ({e}); this guard will not pass blind")
        });
        out.insert(relative_to_plugin(path), bytes);
    }
    assert!(!out.is_empty(), "the plugin directory tracks no files");
    out
}

fn relative_to_plugin(path: &str) -> String {
    path.strip_prefix(&format!("{PLUGIN_DIR}/"))
        .unwrap_or(path)
        .to_string()
}

// ───────────────────── 2. the installer, driven against fixtures ─────────────────────

/// A fake box: a home nobody lives in, a repository nobody committed to before, and a `claude`
/// that does what the mode says and writes down every call.
struct Box_ {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    log: PathBuf,
    tool_log: PathBuf,
    /// Where the installer is told to look for the managed settings that hold the channels door.
    /// The real path is under `/etc` and needs root, so neither branch of that check could be
    /// driven from a test — and the branch nobody can drive is the branch that stays wrong.
    managed: PathBuf,
}

impl Box_ {
    fn new(source_version: &str, server_body: &str) -> Self {
        for tool in ["bash", "git", "jq", "sha256sum", "timeout"] {
            require_tool(tool);
        }
        let dir = tempfile::tempdir().expect("no temporary directory");
        let home = dir.path().join("home");
        let repo = dir.path().join("repo");
        let bin = dir.path().join("bin");
        let log = dir.path().join("claude-calls.log");
        let tool_log = dir.path().join("tool-calls.log");
        let managed = dir.path().join("managed-settings.json");
        fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(&log, "").unwrap();
        fs::write(&tool_log, "").unwrap();

        let me = Box_ {
            _dir: dir,
            home,
            repo,
            bin,
            log,
            tool_log,
            managed,
        };
        me.write_repo(source_version, server_body);
        me.write_fake_claude();
        me.the_bridges_own_test_passes();
        me.the_wire_proof_passes();
        me
    }

    /// The `bun` the installer proves the bridge with. It writes down that it ran, because a proof
    /// nobody can show was run is not a proof.
    fn the_bridges_own_test_passes(&self) {
        self.write_fake(
            "bun",
            "printf 'bun %s\\n' \"$*\" >> \"$FAKE_TOOL_LOG\"\nexit 0\n",
        );
    }

    fn the_bridges_own_test_fails(&self) {
        self.write_fake(
            "bun",
            "printf 'bun %s\\n' \"$*\" >> \"$FAKE_TOOL_LOG\"\n\
             case \"${1:-}\" in install) exit 0 ;; esac\n\
             echo 'the bridge answered a frame the fake hub did not send' >&2\nexit 1\n",
        );
    }

    /// The `cargo` that stands in for the real-bridge-against-the-real-hub proof. It records the
    /// temporary directory it was handed, because that proof binds a Unix socket under it and a
    /// path too long to bind reads as a bridge that disagrees with the hub.
    fn the_wire_proof_passes(&self) {
        self.write_fake(
            "cargo",
            "printf 'cargo TMPDIR=%s\\n' \"${TMPDIR:-}\" >> \"$FAKE_TOOL_LOG\"\n\
             printf 'cargo asked for %s\\n' \"$*\" >> \"$FAKE_TOOL_LOG\"\nexit 0\n",
        );
    }

    /// A `cargo` that passes the wire proof and fails only the older-bridge one, so a test can tell
    /// the two apart. Both were once run by one filter that matched only the first, which is the
    /// defect this models.
    fn the_older_bridge_proof_fails(&self) {
        self.write_fake(
            "cargo",
            "printf 'cargo asked for %s\\n' \"$*\" >> \"$FAKE_TOOL_LOG\"\n\
             case \"$*\" in\n\
               *still_works_against_the_new_hub*)\n\
                 echo 'a bridge from before this change was refused by this hub' >&2\n\
                 exit 101 ;;\n\
             esac\nexit 0\n",
        );
    }

    fn the_wire_proof_fails(&self) {
        self.write_fake(
            "cargo",
            "printf 'cargo TMPDIR=%s\\n' \"${TMPDIR:-}\" >> \"$FAKE_TOOL_LOG\"\n\
             printf 'cargo asked for %s\\n' \"$*\" >> \"$FAKE_TOOL_LOG\"\n\
             echo 'the bridge sent a field the hub does not know' >&2\nexit 101\n",
        );
    }

    fn write_repo(&self, version: &str, server_body: &str) {
        let plugin = self.repo.join(PLUGIN_DIR);
        fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
        fs::create_dir_all(self.repo.join(".claude-plugin")).unwrap();
        fs::create_dir_all(self.repo.join("scripts")).unwrap();
        fs::write(
            self.repo.join(".claude-plugin/marketplace.json"),
            r#"{
  "name": "herdr-tg-local",
  "owner": { "name": "herdr-tg" },
  "plugins": [
    { "name": "kickoff-channel", "source": "./plugins/kickoff-channel" }
  ]
}
"#,
        )
        .unwrap();
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!("{{\n  \"name\": \"kickoff-channel\",\n  \"version\": \"{version}\"\n}}\n"),
        )
        .unwrap();
        fs::write(
            plugin.join("package.json"),
            format!("{{\n  \"name\": \"kickoff-channel\",\n  \"version\": \"{version}\"\n}}\n"),
        )
        .unwrap();
        fs::write(plugin.join("server.ts"), server_body).unwrap();
        fs::write(plugin.join("hub-link.ts"), "// the wire\n").unwrap();
        fs::write(plugin.join("test-against-a-fake-hub.ts"), "// the proof\n").unwrap();

        let installer = repo_root().join("scripts/install-channel-plugin.sh");
        fs::copy(
            &installer,
            self.repo.join("scripts/install-channel-plugin.sh"),
        )
        .unwrap_or_else(|e| panic!("could not copy {}: {e}", installer.display()));

        git(&self.repo, &["init", "-q", "-b", "main"]);
        git(&self.repo, &["add", "-A"]);
        git(
            &self.repo,
            &[
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "user.name=fixture",
                "commit",
                "-q",
                "-m",
                "the fixture",
                "--no-verify",
            ],
        );
    }

    fn write_fake(&self, name: &str, body: &str) {
        let path = self.bin.join(name);
        fs::write(&path, format!("#!/usr/bin/env bash\n{body}")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A `claude` that behaves like the real one on the shapes this installer reads, and lies in
    /// exactly the way `FAKE_CLAUDE_MODE` names. It refuses outright if it is pointed at the real
    /// home, so a fixture that leaks cannot modify the operator's box.
    fn write_fake_claude(&self) {
        let real_home = std::env::var("REAL_HOME_FOR_FIXTURE")
            .ok()
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_default();
        let body = format!(
            r#"set -euo pipefail
if [ "${{HOME:-}}" = "{real_home}" ]; then
  echo "the fixture reached the real home; refusing" >&2
  exit 2
fi
printf '%s\n' "$*" >> "$FAKE_CLAUDE_LOG"
STATE="$HOME/.claude/plugins"
KNOWN="$STATE/known_marketplaces.json"
INSTALLED="$STATE/installed_plugins.json"
mkdir -p "$STATE"
[ -f "$KNOWN" ] || echo '{{}}' > "$KNOWN"
[ -f "$INSTALLED" ] || echo '{{"version":2,"plugins":{{}}}}' > "$INSTALLED"

case "${{1:-}} ${{2:-}}" in
  "plugin marketplace")
    verb="${{3:-}}"; path="${{4:-}}"
    [ "$verb" = "add" ] || {{ echo "unknown marketplace verb $verb" >&2; exit 1; }}
    name="$(jq -r .name "$path/.claude-plugin/marketplace.json")"
    tmp="$(mktemp)"
    jq --arg n "$name" --arg p "$path" \
       '.[$n] = {{source:{{source:"directory",path:$p}},installLocation:$p}}' "$KNOWN" > "$tmp"
    mv "$tmp" "$KNOWN"
    echo "added $name"
    ;;
  "plugin install"|"plugin update")
    spec="${{3:-}}"
    plug="${{spec%%@*}}"; mkt="${{spec##*@}}"
    src="$(jq -r --arg n "$mkt" '.[$n].installLocation' "$KNOWN")/plugins/$plug"
    case "${{FAKE_CLAUDE_MODE:-ok}}" in
      install-fails)
        echo "could not reach the marketplace" >&2
        exit 1 ;;
      install-fails-with-no-final-newline)
        printf 'reading the marketplace\ncould not reach the marketplace' >&2
        exit 1 ;;
      install-does-nothing)
        echo "installed $plug"
        exit 0 ;;
    esac
    ver="$(jq -r .version "$src/.claude-plugin/plugin.json")"
    dest="$STATE/cache/$mkt/$plug/$ver"
    recorded="$ver"
    case "${{FAKE_CLAUDE_MODE:-ok}}" in
      install-into-a-directory-of-its-own-choosing) dest="$STATE/cache/$mkt/$plug/latest" ;;
      install-records-a-version-it-did-not-put-there) recorded="9.9.9" ;;
    esac
    rm -rf "$dest"; mkdir -p "$dest"
    cp -r "$src/." "$dest/"
    if [ "${{FAKE_CLAUDE_MODE:-ok}}" = "install-corrupts" ]; then
      printf '// a byte the source does not have\n' >> "$dest/server.ts"
    fi
    if [ "${{FAKE_CLAUDE_MODE:-ok}}" = "install-drops-a-file" ]; then
      rm -f "$dest/hub-link.ts"
    fi
    if [ "${{FAKE_CLAUDE_MODE:-ok}}" = "install-leaves-a-stray-file" ]; then
      printf '// nothing in the repo knows about this\n' > "$dest/left-behind.ts"
    fi
    tmp="$(mktemp)"
    jq --arg k "$spec" --arg v "$recorded" --arg ip "$dest" --arg c "$(cd "$(jq -r --arg n "$mkt" '.[$n].installLocation' "$KNOWN")" && git rev-parse HEAD)" \
       '.version = 2 | .plugins[$k] = [{{scope:"user",installPath:$ip,version:$v,gitCommitSha:$c,installedAt:"2026-01-01T00:00:00.000Z",lastUpdated:"2026-01-01T00:00:00.000Z"}}]' \
       "$INSTALLED" > "$tmp"
    mv "$tmp" "$INSTALLED"
    echo "installed $plug $recorded"
    ;;
  *)
    echo "the fixture was asked for something it does not model: $*" >&2
    exit 1 ;;
esac
"#
        );
        self.write_fake("claude", &body);
    }

    /// Pre-seed the box as if a previous install had put `version` there with `server_body`.
    fn seed_cache(&self, version: &str, server_body: &str) {
        let dest = self
            .home
            .join(".claude/plugins/cache/herdr-tg-local/kickoff-channel")
            .join(version);
        fs::create_dir_all(dest.join(".claude-plugin")).unwrap();
        fs::write(
            dest.join(".claude-plugin/plugin.json"),
            format!("{{\n  \"name\": \"kickoff-channel\",\n  \"version\": \"{version}\"\n}}\n"),
        )
        .unwrap();
        fs::write(
            dest.join("package.json"),
            format!("{{\n  \"name\": \"kickoff-channel\",\n  \"version\": \"{version}\"\n}}\n"),
        )
        .unwrap();
        fs::write(dest.join("server.ts"), server_body).unwrap();
        fs::write(dest.join("hub-link.ts"), "// the wire\n").unwrap();
        fs::write(dest.join("test-against-a-fake-hub.ts"), "// the proof\n").unwrap();
        fs::write(
            self.home.join(".claude/plugins/installed_plugins.json"),
            format!(
                r#"{{"version":2,"plugins":{{"kickoff-channel@herdr-tg-local":[{{"scope":"user","installPath":"{}","version":"{version}","installedAt":"2026-01-01T00:00:00.000Z","lastUpdated":"2026-01-01T00:00:00.000Z"}}]}}}}"#,
                dest.display()
            ),
        )
        .unwrap();
        self.seed_marketplace(&self.repo.display().to_string());
    }

    fn seed_marketplace(&self, path: &str) {
        fs::write(
            self.home.join(".claude/plugins/known_marketplaces.json"),
            format!(
                r#"{{"herdr-tg-local":{{"source":{{"source":"directory","path":"{path}"}},"installLocation":"{path}"}}}}"#
            ),
        )
        .unwrap();
    }

    fn run_with_mode(&self, mode: &str) -> Ran {
        self.run_args(mode, &[])
    }

    fn run_args(&self, mode: &str, args: &[&str]) -> Ran {
        self.run_in(mode, args, &self._dir.path().display().to_string())
    }

    fn run_in(&self, mode: &str, args: &[&str], tmpdir: &str) -> Ran {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out: Output = Command::new("bash")
            .arg(self.repo.join("scripts/install-channel-plugin.sh"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("TMPDIR", tmpdir)
            .env("HERDR_TG_MANAGED_SETTINGS", &self.managed)
            .env("FAKE_CLAUDE_LOG", &self.log)
            .env("FAKE_TOOL_LOG", &self.tool_log)
            .env("FAKE_CLAUDE_MODE", mode)
            .output()
            .expect("could not start the installer");
        Ran {
            ok: out.status.success(),
            code: out.status.code(),
            text: format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            calls: fs::read_to_string(&self.log).unwrap_or_default(),
            tools: fs::read_to_string(&self.tool_log).unwrap_or_default(),
        }
    }

    /// Everything the fake `bun` and `cargo` recorded, for a test that needs to see what ran rather
    /// than only what the installer said about it.
    fn tool_calls(&self) -> String {
        fs::read_to_string(&self.tool_log).unwrap_or_default()
    }

    /// What the installer wrote down, outside every repository, about what it put on this box.
    fn what_the_box_records(&self) -> String {
        fs::read_to_string(self.home.join(".local/state/herdr-tg/plugin.installed"))
            .unwrap_or_default()
    }
}

struct Ran {
    ok: bool,
    code: Option<i32>,
    text: String,
    calls: String,
    tools: String,
}

impl Ran {
    fn refused(&self, because: &str) {
        assert!(
            !self.ok,
            "the installer exited 0 ({:?}) when it should have refused: {because}\n--- it said ---\n{}\n--- it called ---\n{}",
            self.code, self.text, self.calls
        );
    }
    fn succeeded(&self) {
        assert!(
            self.ok,
            "the installer failed ({:?})\n--- it said ---\n{}\n--- it called ---\n{}",
            self.code, self.text, self.calls
        );
    }
    fn said(&self, needle: &str) {
        assert!(
            self.text.contains(needle),
            "the installer never said {needle:?}\n--- it said ---\n{}",
            self.text
        );
    }
    fn never_said(&self, needle: &str) {
        assert!(
            !self.text.contains(needle),
            "the installer said {needle:?} and must not have\n--- it said ---\n{}",
            self.text
        );
    }
    fn called(&self, needle: &str) {
        assert!(
            self.calls.contains(needle),
            "the installer never called claude {needle:?}\n--- it called ---\n{}",
            self.calls
        );
    }
    fn never_called(&self, needle: &str) {
        assert!(
            !self.calls.contains(needle),
            "the installer called claude {needle:?} and must not have\n--- it called ---\n{}",
            self.calls
        );
    }
    /// A whole line of the tool log, never a prefix of one — `TMPDIR=/tmp` is a prefix of
    /// `TMPDIR=/tmp/somewhere/very/long/indeed`, and a check that cannot tell those apart is a
    /// check that passes on the defect it was written for.
    fn ran_tool(&self, line: &str) {
        assert!(
            self.tools.lines().any(|l| l == line),
            "the installer never ran exactly {line:?}\n--- the tools it ran ---\n{}",
            self.tools
        );
    }
}

#[test]
fn a_failed_plugin_install_is_reported_as_a_failure_not_as_done() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-fails");
    ran.refused("the install command itself failed");
    ran.never_said("verified:");
    // Refusing for SOME reason is not enough. Put the `|| true` back on the `claude` call and the
    // run still ends non-zero — the empty registry catches it two sections later and the operator
    // is told "the install reported success but this box still lists no copy", which is false and
    // sends him looking in the wrong place. The diagnosis is the thing under test.
    ran.said("installing the plugin failed (exit 1)");
    ran.said("could not reach the marketplace");
}

#[test]
fn the_last_line_of_a_failed_commands_output_is_not_swallowed() {
    // The captured output exists so a person can see what went wrong, and many tools end their
    // last line without a newline — which a bare `read` drops. The dropped line is the one that
    // says why.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-fails-with-no-final-newline");
    ran.refused("the install command itself failed");
    ran.said("could not reach the marketplace");
}

#[test]
fn a_temporary_directory_that_is_not_there_is_refused_by_name_before_anything_runs() {
    // An agent session on this box inherits TMPDIR as the literal string `%h/.cache/tmp`, which
    // names no directory at all. The script used to die inside `mktemp` with no sentence of its
    // own, so the reader had to know what a template was to work out what to change.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_in("ok", &[], "%h/.cache/tmp");
    ran.refused("there is nowhere to work");
    ran.said("%h/.cache/tmp");
    ran.said("real absolute directory");
    ran.never_called("plugin install");
}

#[test]
fn the_wire_proof_is_run_where_a_unix_socket_path_still_fits() {
    // The proof binds a Unix socket under TMPDIR, and a socket path is 108 bytes all in. Handing
    // it a long TMPDIR fails the bind, and a failed bind is reported as "the bridge and the hub
    // disagree about the wire" — a false accusation against a bridge that is perfectly correct.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let long = b
        ._dir
        .path()
        .join("a-temporary-directory-with-a-thoroughly-unhelpful-name");
    fs::create_dir_all(&long).unwrap();
    let ran = b.run_in("ok", &[], &long.display().to_string());
    ran.succeeded();
    ran.ran_tool("cargo TMPDIR=/tmp");
}

#[test]
fn an_install_that_exits_zero_but_installed_nothing_is_refused() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-does-nothing");
    ran.refused("nothing was written to the box");
    ran.never_said("verified:");
}

#[test]
fn a_cache_at_the_source_version_with_different_content_is_refused_and_a_bump_is_demanded() {
    let b = Box_::new("0.1.0", "// this week's bridge\n");
    b.seed_cache("0.1.0", "// last week's bridge\n");
    let ran = b.run_with_mode("ok");
    ran.refused("the box already has this version with different bytes");
    ran.said("bump");
    // The whole defect: an install at a version already present is a no-op, so calling it would
    // report success and change nothing.
    ran.never_called("plugin install");
    ran.never_called("plugin update");
}

#[test]
fn a_version_bump_busts_the_cache_and_the_new_cache_is_verified_against_the_source() {
    let b = Box_::new("0.2.0", "// this week's bridge\n");
    b.seed_cache("0.1.0", "// last week's bridge\n");
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    ran.called("plugin update");
    ran.said("verified:");
    ran.said("0.2.0");
    let new_cache = b
        .home
        .join(".claude/plugins/cache/herdr-tg-local/kickoff-channel/0.2.0/server.ts");
    assert_eq!(
        fs::read_to_string(&new_cache).unwrap(),
        "// this week's bridge\n",
        "the bumped version's cache does not hold this week's bridge"
    );
}

#[test]
fn an_installed_copy_that_differs_from_the_source_is_caught_by_the_verification() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-corrupts");
    ran.refused("the copy on the box is not the copy in the repository");
    ran.said("server.ts");
    ran.never_said("verified:");
}

#[test]
fn a_second_run_with_nothing_changed_makes_no_install_call_and_still_verifies() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let first = b.run_with_mode("ok");
    first.succeeded();
    fs::write(&b.log, "").unwrap();
    let second = b.run_with_mode("ok");
    second.succeeded();
    second.said("verified:");
    second.never_called("plugin install");
    second.never_called("plugin update");
}

#[test]
fn a_marketplace_registered_at_another_checkout_is_refused_not_overwritten() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let elsewhere = b._dir.path().join("another-checkout");
    fs::create_dir_all(&elsewhere).unwrap();
    b.seed_marketplace(&elsewhere.display().to_string());
    let ran = b.run_with_mode("ok");
    ran.refused("the marketplace name is taken by another checkout");
    ran.said(&elsewhere.display().to_string());
    ran.said("marketplace remove");
    ran.never_called("marketplace add");
}

#[test]
fn a_plugin_tree_with_uncommitted_edits_is_refused_and_the_flag_is_the_only_way_past_it() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    // A directory marketplace copies the WORKING TREE, so an uncommitted edit here is an
    // uncommitted edit on the box — and "installed <commit>" would name a commit that does not
    // contain what was installed.
    fs::write(
        b.repo.join(PLUGIN_DIR).join("server.ts"),
        "// an edit nobody committed\n",
    )
    .unwrap();

    let refused = b.run_args("ok", &[]);
    refused.refused("the tree it would copy is not one anyone can get back to");
    refused.said("server.ts");
    refused.never_called("plugin install");

    // The operator can still say "I know, do it anyway" — and then what got installed is described
    // as what it is, rather than as a commit.
    let anyway = b.run_args("ok", &["--allow-dirty"]);
    anyway.succeeded();
    anyway.said("verified:");
    anyway.said("uncommitted");
}

#[test]
fn the_verified_block_does_not_claim_bytes_it_never_compared() {
    // Measured against the real `claude` under a throwaway home on 8 September 2026: a directory
    // marketplace copies the WHOLE plugin directory, gitignored files included — here 35 MB of
    // JavaScript the bridge imports at run time. `git ls-files` never names those, so neither
    // fingerprint looks at them, and the script still announced "N of N match, byte for byte" over
    // a box holding bytes it had never seen. What was not compared has to say so.
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::write(b.repo.join(".gitignore"), "node_modules/\n").unwrap();
    let dependency = b.repo.join(PLUGIN_DIR).join("node_modules/leftover");
    fs::create_dir_all(&dependency).unwrap();
    fs::write(
        dependency.join("index.js"),
        "// a dependency nobody tracks\n",
    )
    .unwrap();
    git(&b.repo, &["add", "-A"]);
    git(
        &b.repo,
        &[
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "user.name=fixture",
            "commit",
            "-q",
            "-m",
            "the dependencies are not tracked",
            "--no-verify",
        ],
    );

    let ran = b.run_with_mode("ok");
    ran.succeeded();
    assert!(
        b.home
            .join(".claude/plugins/cache/herdr-tg-local/kickoff-channel/0.1.0/node_modules/leftover/index.js")
            .is_file(),
        "the fixture must model the real copy: an ignored file reaches the box too"
    );
    ran.said("not compared");
    ran.said("node_modules");
}

#[test]
fn a_file_on_the_box_that_this_repo_does_not_track_is_named_rather_than_counted_as_a_match() {
    // The other half: a file that is neither tracked nor a dependency is a file nobody in this
    // repository can account for, and a bridge with an extra module in it is not this bridge.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-leaves-a-stray-file");
    ran.refused("the box holds a file this repo has never seen");
    ran.said("left-behind.ts");
    ran.never_said("verified:");
}

#[test]
fn a_tracked_file_the_install_did_not_produce_is_refused_rather_than_skipped() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-drops-a-file");
    ran.refused("a file this repo tracks never reached the box");
    ran.said("hub-link.ts");
    ran.never_said("verified:");
}

#[test]
fn the_same_checkout_reached_through_a_symlink_is_not_called_another_checkout() {
    // `claude plugin marketplace add` records the path it was GIVEN. One run through a symlink and
    // the next through the real path then disagree, and the operator is told his own checkout
    // belongs to somebody else — with a `remove` command that would undo a registration that is
    // perfectly correct.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let by_another_name = b._dir.path().join("repo-by-another-name");
    std::os::unix::fs::symlink(&b.repo, &by_another_name).unwrap();
    b.seed_marketplace(&by_another_name.display().to_string());
    let ran = b.run_with_mode("ok");
    ran.never_said("belongs to another checkout");
    ran.succeeded();
}

#[test]
fn channels_switched_off_in_managed_settings_is_not_reported_as_an_open_door() {
    // The door was read with a search for the WORD `channelsEnabled`, which a `false` contains as
    // happily as a `true`. The operator was then told the door was open, and every session he
    // started afterwards was deaf for a reason the install had just told him was fine.
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::write(&b.managed, "{ \"channelsEnabled\": false }\n").unwrap();
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    ran.never_said("The door is open");
    ran.said("THE DOOR IS STILL SHUT");
}

#[test]
fn a_door_that_is_open_is_said_to_be_open() {
    // The matching half, so the check above cannot be satisfied by never seeing a door at all.
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::write(&b.managed, "{ \"channelsEnabled\": true }\n").unwrap();
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    ran.said("The door is open");
}

#[test]
fn a_registry_written_in_a_way_this_script_does_not_know_is_refused_not_guessed_at() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    b.seed_marketplace(&b.repo.display().to_string());
    fs::write(
        b.home.join(".claude/plugins/installed_plugins.json"),
        "{\"version\":3,\"plugins\":{}}\n",
    )
    .unwrap();
    let ran = b.run_with_mode("ok");
    ran.refused("the registry is written in a shape this script has never read");
    ran.never_called("plugin install");
}

#[test]
fn a_box_that_lists_two_copies_under_one_name_is_told_that_and_not_told_it_has_none() {
    // One sentence used to cover "no copies" and "two copies", and it said "no". A person sent
    // looking for a missing install when the box has two of them looks in the wrong place.
    let b = Box_::new("0.1.0", "// the bridge\n");
    b.seed_marketplace(&b.repo.display().to_string());
    fs::write(
        b.home.join(".claude/plugins/installed_plugins.json"),
        "{\"version\":2,\"plugins\":{\"kickoff-channel@herdr-tg-local\":[\
         {\"scope\":\"user\",\"installPath\":\"/nowhere/a\",\"version\":\"0.1.0\"},\
         {\"scope\":\"user\",\"installPath\":\"/nowhere/b\",\"version\":\"0.1.0\"}]}}\n",
    )
    .unwrap();
    let ran = b.run_with_mode("ok");
    ran.refused("the box lists two copies under one name");
    ran.said("two copies");
    ran.never_said("no copy");
    ran.never_called("plugin install");
}

#[test]
fn a_copy_stored_under_a_directory_that_is_not_its_version_is_refused() {
    // The cache directory is keyed by the version. A copy stored anywhere else means the next
    // install at the next version lands somewhere nobody is reading.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-into-a-directory-of-its-own-choosing");
    ran.refused("the copy is not stored under its own version");
    ran.never_said("verified:");
}

#[test]
fn a_box_that_records_a_version_it_did_not_install_is_refused() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("install-records-a-version-it-did-not-put-there");
    ran.refused("the box now records a version this repo does not have");
    ran.said("9.9.9");
    ran.never_said("verified:");
}

#[test]
fn a_marketplace_that_offers_the_plugin_from_another_directory_is_refused() {
    // Everything after this fingerprints one directory. If the marketplace offers a different one
    // we would check one tree and install another, and the check would pass over bytes nobody read.
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::write(
        b.repo.join(".claude-plugin/marketplace.json"),
        "{\n  \"name\": \"herdr-tg-local\",\n  \"owner\": { \"name\": \"herdr-tg\" },\n  \
         \"plugins\": [ { \"name\": \"kickoff-channel\", \"source\": \"./somewhere-else\" } ]\n}\n",
    )
    .unwrap();
    let ran = b.run_with_mode("ok");
    ran.refused("the marketplace offers a directory this script never looked at");
    ran.said("somewhere-else");
    ran.never_called("plugin install");
}

#[test]
fn two_manifests_that_disagree_about_the_version_are_refused() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::write(
        b.repo.join(PLUGIN_DIR).join("package.json"),
        "{\n  \"name\": \"kickoff-channel\",\n  \"version\": \"0.9.0\"\n}\n",
    )
    .unwrap();
    let ran = b.run_args("ok", &["--allow-dirty"]);
    ran.refused("the two manifests spell one version two ways");
    ran.said("0.9.0");
    ran.never_called("plugin install");
}

#[test]
fn the_install_refuses_when_the_bridge_and_the_hub_disagree_and_installs_nothing() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    b.the_wire_proof_fails();
    let ran = b.run_with_mode("ok");
    ran.refused("the wire proof failed");
    ran.said("the bridge and the hub disagree about the wire; not installing it");
    ran.said("the bridge sent a field the hub does not know");
    ran.never_called("plugin install");
}

#[test]
fn a_bridge_that_fails_its_own_test_is_not_installed() {
    let b = Box_::new("0.1.0", "// the bridge\n");
    b.the_bridges_own_test_fails();
    let ran = b.run_with_mode("ok");
    ran.refused("the bridge's own test failed");
    ran.said("the bridge answered a frame the fake hub did not send");
    ran.never_called("plugin install");
}

#[test]
fn the_install_asks_for_the_scope_it_later_reads() {
    // Everything this script reads out of the registry is filtered to the `user` scope. The writer
    // relied on that being the default; the reader and the writer must not be able to drift.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    ran.called("--scope user");
}

#[test]
fn what_was_installed_is_written_down_outside_every_repository() {
    // The terminal that ran the install is not a record. Something has to be able to answer "which
    // bridge is on this box" tomorrow, and it cannot live in a repo — hub state in a working tree
    // is state somebody's coordinator commits.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    let recorded = b.what_the_box_records();
    for expected in ["kickoff-channel", "0.1.0", "server.ts"] {
        assert!(
            recorded.contains(expected),
            "what the box records does not mention {expected:?}:\n{recorded}"
        );
    }
    assert!(
        !b.repo.join("plugins/kickoff-channel/.installed").exists(),
        "nothing about the install may be written into the repository it came from"
    );
}

#[test]
fn an_install_of_uncommitted_bytes_is_recorded_as_bytes_and_not_only_as_a_commit() {
    // `--allow-dirty` puts bytes on a box that no commit contains. Naming only the commit makes two
    // boxes look identical while they hold different bridges, so the identity has to carry
    // something that moves with the bytes.
    // Two boxes, the same version and the same commit, and different bridges on them.
    let honest = Box_::new("0.1.0", "// the bridge\n");
    honest.run_with_mode("ok").succeeded();
    let honest_record = honest.what_the_box_records();

    let edited = Box_::new("0.1.0", "// the bridge\n");
    fs::write(
        edited.repo.join(PLUGIN_DIR).join("server.ts"),
        "// an edit nobody committed\n",
    )
    .unwrap();
    let ran = edited.run_args("ok", &["--allow-dirty"]);
    ran.succeeded();
    ran.said("uncommitted");
    ran.said("contents");
    let edited_record = edited.what_the_box_records();
    assert!(
        edited_record.contains("uncommitted"),
        "the record does not say the bytes are in no commit:\n{edited_record}"
    );
    assert_ne!(
        contents_line(&honest_record),
        contents_line(&edited_record),
        "two different bridges were recorded under the same identity; nothing can tell one box \
         from the other"
    );
}

/// The line of the record that names the bytes, so a test can compare two installs without
/// depending on the rest of the wording.
fn contents_line(record: &str) -> String {
    record
        .lines()
        .find(|l| l.starts_with("contents"))
        .unwrap_or_else(|| panic!("the record has no line naming the bytes:\n{record}"))
        .to_string()
}

#[test]
fn a_file_this_repo_tracks_but_does_not_have_on_disk_stops_the_install() {
    // The fingerprint writes a placeholder for a file it could not find, so a gap on ONE side can
    // never line up with a gap on the other and read as a match. Without it, a file that is tracked
    // and missing is simply absent from both lists, the comparison succeeds over nothing, and a
    // bridge with a module missing is announced as verified.
    let b = Box_::new("0.1.0", "// the bridge\n");
    fs::remove_file(b.repo.join(PLUGIN_DIR).join("hub-link.ts")).unwrap();
    let ran = b.run_args("ok", &["--allow-dirty"]);
    ran.refused("a file this repo tracks is not in the working tree");
    ran.said("incomplete");
    ran.never_called("plugin install");
}

#[test]
fn the_proof_that_an_older_bridge_still_works_is_one_of_the_proofs_the_install_runs() {
    // Two properties, two tests, and one filter used to run only the first: that the bridge in this
    // repo agrees with the hub in this repo, and that a bridge from BEFORE this change still does.
    // The second is the compatibility promise every box already carrying a bridge depends on, and
    // its name shares no substring with the other, so the filter walked straight past it — an
    // install could be called verified with the promise never once checked.
    let b = Box_::new("0.1.0", "// the bridge\n");
    let ran = b.run_with_mode("ok");
    ran.succeeded();
    assert!(
        b.tool_calls()
            .contains("a_bridge_from_before_this_change_still_works_against_the_new_hub"),
        "the install never asked for the older-bridge proof\n--- the tools it ran ---\n{}",
        b.tool_calls()
    );
}

#[test]
fn a_hub_that_has_stopped_accepting_older_bridges_is_not_installed_over() {
    // And the refusal, in the words that say which of the two proofs failed — "the bridge and the
    // hub disagree about the wire" would send a person to read this repo's own bridge, which is
    // fine, when what broke is every bridge already out on a box.
    let b = Box_::new("0.1.0", "// the bridge\n");
    b.the_older_bridge_proof_fails();
    let ran = b.run_with_mode("ok");
    ran.refused("this hub turns away a bridge from before this change");
    ran.said("a bridge from before this change was refused by this hub");
    ran.never_called("plugin install");
}

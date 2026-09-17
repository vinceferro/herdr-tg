//! **Nothing that arrives from outside can cause a process to exist.**
//!
//! Four documents state something stronger, and it is false as written. `docs/INTERFACES.md`
//! (*"There is no `Command` in the binary"*, and *"Zero `Command` in the binary"* in seam ④),
//! `docs/CAPABILITIES.md` (*"there is no `Command` in this binary at all"*, and *"The hub has no
//! `Command` in the binary today"* at OPEN 1) and `docs/HUB-AND-KICKOFF.md` (*"zero `Command` in
//! the binary"*) all claim the binary holds none. It holds three, in enrolment, and they are the
//! reason enrolment can tell an operator that his project's secret is about to be committed.
//!
//! An absolute claim that the tree does not meet is worse than a narrower one that it does: the
//! next person to read those sentences and then read `cmd/enroll.rs` learns that the documents are
//! not to be trusted, and the sentence that mattered goes with them. So this file asserts the
//! property that is TRUE, and it is the one the doors were designed around:
//!
//! **A Telegram message, a tap on a keyboard the hub drew, or a frame from a bridge cannot reach
//! anything that starts a process.** Not by naming a program, not by choosing one, not by
//! supplying an argument to one. The enrolment call sites are reached from `argv` at the terminal
//! and nowhere else; they name `git` as a literal and pass a fixed subcommand; and no file that
//! inbound content flows through — the door, the hub, the bot, the surface, the pacer, the
//! registry — names a way to start anything at all.
//!
//! # What it checks
//!
//! 1. **A short, named allowlist of files may spawn.** Four, each with the reason it is allowed.
//!    Every other source file in every workspace member must name no way to start a process.
//! 2. **Every one of those files must still hold what it is allowed for.** The programs are
//!    declared by name and each must be found; a file that no longer spawns anything is a file
//!    whose exemption has to be re-decided rather than left standing over whatever takes the name.
//! 3. **The three sites that SHIP are counted, and their subcommands named.** A fourth `git` in
//!    the shipped half of `cmd/enroll.rs` turns this red, so adding one is a deliberate act that
//!    shows up in a diff and not a line nobody notices.
//! 4. **Every spawn names a FIXED program.** A string literal, or this same binary via
//!    `std::env::current_exe()`. A program assembled from a value is refused even inside an
//!    allowed file — that is the shape a string from the wire would have to take to become a
//!    command line.
//! 5. **Nothing on the inbound path spawns at all**, outside its own `#[cfg(test)]` block. Those
//!    files are named one by one rather than matched by a pattern, so renaming one of them fails
//!    here instead of quietly leaving it unwatched.
//!
//! # This guard FAILS when it cannot look
//!
//! The sibling guard in `crates/herdr-client/tests/no_live_write_call_site.rs` was walked past six
//! separate ways, and nearly every one ended the same way: a lookup came up empty and the code
//! carried on as though it had found nothing to report. Empty is not the same as clean. So:
//!
//! * A workspace member that contributes no source file is a FAILURE. So is a `src/` that cannot
//!   be listed, an entry whose kind cannot be determined, and a directory reached by a symbolic
//!   link — a link leads out of the tree this rule is written about.
//! * **Every allowlisted file and every inbound-path file must be IN the walk**, by name. A
//!   renamed file, a module moved out from under `src/`, or a member dropped from the manifest
//!   does not read as "it spawns nothing"; it reads as "this guard has no idea".
//! * `include!` and `#[path]` put code in a crate from a file the walk never opens, and two of the
//!   six ways past the sibling guard were exactly those. They are refused outright, with ONE named
//!   exception whose text is pinned and whose target is scanned as though it were under `src/`.
//! * The comment stripper reports an unterminated comment or string rather than guessing, and
//!   brace matching runs over a copy with string bodies blanked as well, so a `"{"` in a format
//!   string cannot move the end of a test module and take half a file out of the scan with it.
//! * A denied spelling that matches nothing in the tree today — `posix_spawn`, `libc::fork` — is a
//!   rule for tomorrow, and `every_spelling_this_guard_denies_is_one_it_can_actually_find` is what
//!   keeps it from silently being a typo.
//!
//! # Comments are prose; string literals are code
//!
//! The scan runs with comments blanked and string literals left in place, for the same reason the
//! transport guard does it: a comment saying "the binary starts nothing" is the code being well
//! explained, while `"git"` in a string IS the program being named.
//!
//! It reads files. It starts nothing — deliberately, in the file that says nothing may.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ───────────────────────────────────────────────────────────────────────────────────────────────
// What may start a process, and why.

/// Whether an allowed file's sites reach the shipped binary, and what that costs it.
#[derive(Clone, Copy)]
enum Ships {
    /// Some of the sites here are compiled into the binary the operator runs.
    ///
    /// `sites` is the number of them OUTSIDE this file's own `#[cfg(test)]` blocks and is exact:
    /// a fourth goes red. `argv_heads` names the subcommand each shipped site passes, so the scan
    /// fails if one of them is swapped for another rather than merely counted.
    Yes {
        sites: usize,
        argv_heads: &'static [&'static str],
    },
    /// Every site in this file must sit inside a `#[cfg(test)]` block IN THIS FILE. The shipped
    /// half of it starts nothing, and the scan proves that rather than taking the file's word.
    OnlyUnderCfgTest,
    /// The whole file is a test module, gated where it is DECLARED rather than inside itself.
    ///
    /// The declaration is pinned: drop the `#[cfg(test)]` from it and the file starts shipping,
    /// with a dozen programs in it and this guard otherwise none the wiser.
    WholeFileGatedAt {
        declared_in: &'static str,
        declaration: &'static str,
    },
}

/// One file that may start a process, the programs it may name, and the reason it is allowed.
struct MayStartAProcess {
    file: &'static str,
    ships: Ships,
    /// Exactly the programs this file may name — nothing else, and every one of these must still
    /// be found there. Sorted, because the scan compares it against a sorted set.
    programs: &'static [&'static str],
    why: &'static str,
}

/// How a spawn may name this same binary. It is the one program that is fixed without being a
/// literal: whatever is already running, which no inbound string can influence.
const THIS_BINARY: &str = "std::env::current_exe()";

/// The whole allowlist. Four files. Adding a fifth is a decision about what this product does.
const MAY_START_A_PROCESS: [MayStartAProcess; 4] = [
    MayStartAProcess {
        file: "crates/herdr-tg/src/cmd/enroll.rs",
        ships: Ships::Yes {
            sites: 3,
            argv_heads: &["rev-parse", "check-ignore", "ls-files"],
        },
        programs: &["git"],
        why: "Enrolment asks git whether this project's secret would be committed, because the \
              hand-rolled version read one .gitignore at the repo root and stayed silent for the \
              shape where the danger is greatest. Reached from `main.rs`'s `Cmd::Enroll` and \
              nowhere else — argv, at the terminal, where a person is. Three sites ship: is this \
              folder in a working tree at all (`rev-parse`), would the secret be committed \
              (`check-ignore`), or is it tracked already (`ls-files`), where the advice about a \
              .gitignore rule would be wrong. Two more build a real repo inside this file's own \
              test module.",
    },
    MayStartAProcess {
        file: "crates/herdr-tg/src/presence.rs",
        ships: Ships::OnlyUnderCfgTest,
        programs: &["/bin/true"],
        why: "One test needs a pid that is certainly not a process, so it starts `/bin/true`, \
              reaps it, and uses the number it had. Inventing a number instead would prove \
              nothing, because the number might be alive. The shipped half writes a file and reads \
              a lock; it starts nothing.",
    },
    MayStartAProcess {
        file: "crates/herdr-tg/src/summarize.rs",
        ships: Ships::OnlyUnderCfgTest,
        programs: &[THIS_BINARY],
        why: "The ambient-proxy test has to run in a child: reqwest reads the proxy variables once \
              and caches the answer for the life of the process, so setting them in-process after \
              some other test has already built a client would pass while proving nothing. It \
              re-runs THIS test binary with one ignored test named — a program no value chooses, \
              because it is the one already running.",
    },
    MayStartAProcess {
        file: "crates/herdr-tg/src/hub/tests.rs",
        ships: Ships::WholeFileGatedAt {
            declared_in: "crates/herdr-tg/src/hub.rs",
            declaration: "#[cfg(test)] mod tests;",
        },
        programs: &["/bin/true", "bun", "git", "mkfifo", "sh"],
        why: "The hub's own test module, and the one file under the hub that is excused from this \
              rule — the same exemption `the_hub_does_not_know_what_a_socket_is.rs` makes for the \
              same file and the same reason: it drives the real thing on purpose. It starts the \
              real bun bridge, builds real git repos, makes a fifo the read loop can block on, and \
              reaps a child to get a dead pid. None of it is in the shipped binary.",
    },
];

/// The files inbound content flows through, named one at a time.
///
/// A pattern would have been shorter and would have gone quiet the day one of these was renamed.
/// Each must EXIST and must name no way to start a process outside its own `#[cfg(test)]` block —
/// which is the sentence the documents should be making, so it is the sentence that is asserted.
///
/// `transport.rs` is the door bytes arrive at; `hub.rs` and everything under `src/hub/` decide what
/// they mean; `bot.rs` is Telegram in both directions; `surface.rs` and `render.rs` turn a frame
/// into what a phone shows; `queue.rs` is the pacer that spends the send budget; `registry.rs`,
/// `conversations.rs` and `presence.rs` are identity and what is live; `summarize.rs` is the only
/// thing that sends anything off this machine; `media.rs` and `heartbeat.rs` are reached from the
/// serving path too. `hub/door.rs` is the ring of operator-visible events the hub appends for the
/// gateway that will one day serve them off this machine — a file a frame's own words land in, so
/// it is on this list exactly as the files around it are. `hub/answers.rs` is the other half of
/// that door: the drop an operator's taps and typed words arrive IN as files, and the sweep that
/// turns them into frames — inbound content in file form, pure routing, watched like the rest.
const THE_INBOUND_PATH: [&str; 14] = [
    "crates/herdr-tg/src/bot.rs",
    "crates/herdr-tg/src/conversations.rs",
    "crates/herdr-tg/src/heartbeat.rs",
    "crates/herdr-tg/src/hub.rs",
    "crates/herdr-tg/src/hub/answers.rs",
    "crates/herdr-tg/src/hub/door.rs",
    "crates/herdr-tg/src/media.rs",
    "crates/herdr-tg/src/presence.rs",
    "crates/herdr-tg/src/queue.rs",
    "crates/herdr-tg/src/registry.rs",
    "crates/herdr-tg/src/render.rs",
    "crates/herdr-tg/src/summarize.rs",
    "crates/herdr-tg/src/surface.rs",
    "crates/herdr-tg/src/transport.rs",
];

/// Spellings that mean "a process could begin here".
///
/// Matched as plain substrings of the comment-free source. Deliberately NOT in the list: the bare
/// word `Command`, because `bot.rs` has held a `pub enum Command` — the two Telegram commands —
/// since before the hub existed, and a rule that tripped on it would be switched off within a
/// week. `Command::new` and `process::Command` cannot match that enum, its `parse`, or teloxide's
/// `BotCommands`.
///
/// The families:
///
/// * `Command::new` / `process::Command` — the constructor and the type, whatever path leads to
///   them, `std` and `tokio` alike.
/// * `tokio::process` — the module, so a `Child` held in a struct field counts as much as the
///   `Command` that made it.
/// * `Stdio` — a `Command`'s plumbing. It cannot appear without one.
/// * `CommandExt` / `pre_exec` — `std::os::unix::process::CommandExt` carries `exec()`, which
///   replaces this process rather than starting one beside it, and `pre_exec`, which runs code in
///   the child between fork and exec.
/// * `posix_spawn` / `execvp` / `execve` / `libc::fork` / `libc::system` / `libc::popen` — the raw
///   syscalls, none of which the tree uses today. They are here for the day someone reaches past
///   `std` for one, and `every_spelling_this_guard_denies_is_one_it_can_actually_find` is what
///   keeps them from being unfindable typos in the meantime.
/// * `current_exe` — naming this binary is how a program starts a second copy of itself.
const NOTHING_HERE_STARTS_A_PROCESS: [&str; 13] = [
    "Command::new",
    "process::Command",
    "tokio::process",
    "Stdio",
    "CommandExt",
    "pre_exec",
    "posix_spawn",
    "execvp",
    "execve",
    "libc::fork",
    "libc::system",
    "libc::popen",
    "current_exe",
];

/// Ways to put code in a crate from a file this walk never opens.
///
/// `include!("…")` splices a file into the module it sits in; `#[path = "…"]` mounts a module from
/// anywhere on disk, and `cfg_attr` can carry that conditionally. Two of the six ways past the
/// sibling guard were exactly these. Refused outright rather than resolved: a resolver is an arms
/// race, and `write-guard-stays-a-scanner` records the operator's ruling that this guard stays a
/// scanner and fixes what an ordinary commit could stumble into.
const NO_CODE_FROM_ELSEWHERE: [&str; 3] = ["include!", "#[path", "cfg_attr"];

/// The one redirect in the tree, pinned by its exact text and followed on purpose.
///
/// `herdr-client` wires its offline herdr stand-in in twice: integration tests declare their own
/// `mod support;`, and the crate itself mounts the same file so the crate-private `transport`
/// module can be tested without widening the public API. It is real code in a real crate, so it is
/// scanned like anything under `src/` — and if its text changes, this guard wants to be told
/// rather than to keep following a path that no longer exists.
struct Redirect {
    in_file: &'static str,
    declaration: &'static str,
    pulls_in: &'static str,
}

const THE_ONE_REDIRECT: Redirect = Redirect {
    in_file: "crates/herdr-client/src/lib.rs",
    declaration: "#[path = \"../tests/support/mod.rs\"]",
    pulls_in: "crates/herdr-client/tests/support/mod.rs",
};

/// Manifest sections that can point a compiled target at a directory this walk does not read.
///
/// None of the three members declares one today, so the rule costs nothing. The day one appears,
/// this is where the decision about where the walk goes gets made rather than the place it gets
/// missed: a `[[bin]]` with a `path` outside `src/` is a shipped program the scan would never open.
const NO_TARGET_SOMEWHERE_ELSE: [&str; 4] = ["[[bin]]", "[lib]", "[[example]]", "[[bench]]"];

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Looking, and failing when it cannot.

/// The workspace root: `crates/herdr-tg/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-tg sits two levels below the workspace root")
        .to_path_buf()
}

/// Read one file, or say why the guard is blind. Every caller turns the `Err` into a failing scan.
fn read_source(root: &Path, relative: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(relative)).map_err(|e| {
        format!(
            "{relative} could not be read ({e}). This guard proves nothing about a file it cannot \
             open, so it fails instead of reporting that nothing in it starts a process."
        )
    })
}

/// The workspace members, read out of the root manifest.
///
/// Parsed rather than hardcoded, because a crate added tomorrow must be covered the day it is
/// added and not the day someone remembers to come back here. A manifest that will not yield a
/// members list is a problem, never an empty list — that is the whole failure this file is written
/// against.
fn workspace_members(root: &Path) -> Result<Vec<String>, String> {
    let manifest = read_source(root, "Cargo.toml")?;
    let (code, _) = strip("Cargo.toml", &manifest)?;
    let Some(key) = code.find("members") else {
        return Err(
            "the workspace manifest names no `members`, so this guard does not know which crates \
             it is meant to be reading."
                .to_string(),
        );
    };
    let Some(open) = code[key..].find('[') else {
        return Err(
            "`members` in the workspace manifest is not a list this guard can read.".into(),
        );
    };
    let open = key + open;
    let Some(close) = code[open..].find(']') else {
        return Err("the `members` list in the workspace manifest never closes.".into());
    };

    let mut members = Vec::new();
    let inside = &code[open + 1..open + close];
    let mut rest = inside;
    while let Some(q) = rest.find('"') {
        let after = &rest[q + 1..];
        let Some(end) = after.find('"') else {
            return Err("a member name in the workspace manifest never closes its quote.".into());
        };
        members.push(after[..end].to_string());
        rest = &after[end + 1..];
    }

    if members.is_empty() {
        return Err(
            "the workspace manifest's `members` list is empty, which would make this whole scan \
             vacuous."
                .to_string(),
        );
    }
    members.sort();
    Ok(members)
}

/// Every `.rs` file under one directory, workspace-relative and sorted, with **every failure to
/// look returned as a problem** rather than as an empty list.
fn rust_files_under(root: &Path, dir_relative: &str) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();

    let top = root.join(dir_relative);
    if !top.is_dir() {
        problems.push(format!(
            "{dir_relative} is not a directory this guard can walk. A crate whose sources have \
             moved is not a crate that starts nothing; it is one this scan never read."
        ));
        return (files, problems);
    }

    let mut stack = vec![top];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                problems.push(format!(
                    "{} could not be listed ({e}); this guard fails rather than reporting that \
                     nothing in there starts a process.",
                    dir.display()
                ));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    problems.push(format!(
                        "an entry of {} could not be read ({e}); this guard fails rather than \
                         skipping it.",
                        dir.display()
                    ));
                    continue;
                }
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_symlink() => problems.push(format!(
                    "{} is a symbolic link, and this guard will not walk through one. `file_type` \
                     is lstat-based, so a link to a directory answers neither `is_dir` nor `.rs` \
                     and a walk that asks only those two questions goes past it in silence — which \
                     is how the sibling guard was walked past. Put the file under {dir_relative} \
                     itself, or leave it out.",
                    path.display()
                )),
                Ok(_) if path.extension().is_some_and(|e| e == "rs") => {
                    files.push(
                        path.strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
                Ok(_) => {}
                Err(e) => problems.push(format!(
                    "{} could not be identified as a file or a directory ({e}); this guard fails \
                     rather than guessing.",
                    path.display()
                )),
            }
        }
    }
    files.sort();
    (files, problems)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Telling code from prose.

/// The source twice over, byte-for-byte the same length so offsets and line numbers still line up:
///
/// * **code** — comments blanked, string literals left exactly where they were. What the scan reads.
/// * **skeleton** — comments AND the bodies of string and char literals blanked. What brace
///   matching reads, because a lone `"{"` in a format string would otherwise move the end of a
///   `#[cfg(test)]` module and take the rest of a file out of the scan without a word.
///
/// Reports an unterminated comment or string rather than guessing. In a file that compiles that
/// cannot happen, and if it somehow does, this guard has lost track of where the code is.
fn strip(relative: &str, src: &str) -> Result<(String, String), String> {
    let b = src.as_bytes();
    let mut code = b.to_vec();
    let mut skeleton = b.to_vec();
    let mut i = 0;

    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let from = i;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                blank(&mut code, from, i);
                blank(&mut skeleton, from, i);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let from = i;
                let opened_at = line_of(src, i);
                let mut depth = 0usize;
                loop {
                    if i >= b.len() {
                        return Err(format!(
                            "a block comment opened at {relative}:{opened_at} never closes; this \
                             guard cannot tell code from prose in that file."
                        ));
                    }
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                blank(&mut code, from, i);
                blank(&mut skeleton, from, i);
            }
            // `r"…"`, `r#"…"#`, `br#"…"#` — a raw string swallows quotes and backslashes, so
            // reading one as an ordinary string leaves the scanner inside a span that never closes
            // where the code thinks it does.
            b'r' | b'b' if !is_ident_byte(i.checked_sub(1).map(|p| b[p])) => {
                match raw_string_len(b, i) {
                    Some(n) => {
                        blank(&mut skeleton, i, i + n);
                        i += n;
                    }
                    None => i += 1,
                }
            }
            b'"' => {
                let end = end_of_string(relative, src, i)?;
                blank(&mut skeleton, i, end);
                i = end;
            }
            // A char literal can hold a quote (`'"'`) or a brace (`'{'`); a lifetime (`'a`) looks
            // almost the same and holds neither. Getting this wrong opens a span that eats the rest
            // of the file, which is how a scanner goes quietly blind.
            b'\'' => match char_literal_len(src, i) {
                Some(n) => {
                    blank(&mut skeleton, i, i + n);
                    i += n;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }

    let code = String::from_utf8(code).map_err(|_| {
        format!("blanking {relative} split a character; this guard will not scan the result.")
    })?;
    let skeleton = String::from_utf8(skeleton).map_err(|_| {
        format!("blanking {relative} split a character; this guard will not scan the result.")
    })?;
    Ok((code, skeleton))
}

/// Spaces over a span, leaving newlines alone so line numbers survive.
fn blank(buf: &mut [u8], from: usize, to: usize) {
    let to = to.min(buf.len());
    for byte in &mut buf[from..to] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

/// Is the byte before an `r`/`b` part of an identifier? `for` and `char` end in those letters, and
/// a raw-string prefix is only a prefix when nothing is glued to its left.
fn is_ident_byte(b: Option<u8>) -> bool {
    matches!(b, Some(c) if c.is_ascii_alphanumeric() || c == b'_')
}

/// 1-based line number of a byte offset, for a failure that names a place.
fn line_of(src: &str, at: usize) -> usize {
    src[..at].bytes().filter(|&c| c == b'\n').count() + 1
}

/// Offset just past the closing quote of the ordinary string starting at `start`.
fn end_of_string(relative: &str, src: &str, start: usize) -> Result<usize, String> {
    let b = src.as_bytes();
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err(format!(
        "a string opened at {relative}:{} never closes; this guard cannot tell code from text in \
         that file.",
        line_of(src, start)
    ))
}

/// Length of the raw string starting at `start`, or `None` if this is not one.
fn raw_string_len(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if b.get(i) == Some(&b'b') {
        i += 1;
    }
    if b.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes = {
        let from = i;
        while b.get(i) == Some(&b'#') {
            i += 1;
        }
        i - from
    };
    if b.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < b.len() {
        if b[i] == b'"'
            && b[i + 1..]
                .iter()
                .take(hashes)
                .filter(|&&c| c == b'#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes - start);
        }
        i += 1;
    }
    None
}

/// Length of the char literal starting at `start`, or `None` if it is a lifetime.
fn char_literal_len(src: &str, start: usize) -> Option<usize> {
    let b = src.as_bytes();
    if b.get(start + 1) == Some(&b'\\') {
        let mut i = start + 2;
        while i < b.len() && i < start + 16 {
            if b[i] == b'\'' {
                return Some(i + 1 - start);
            }
            i += 1;
        }
        return None;
    }
    let c = src[start + 1..].chars().next()?;
    let after = start + 1 + c.len_utf8();
    if b.get(after) == Some(&b'\'') {
        Some(after + 1 - start)
    } else {
        None
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Which code ships.

const CFG_TEST: &str = "#[cfg(test)]";

/// The byte spans this file's own `#[cfg(test)]` blocks cover.
///
/// Only the exact spelling `#[cfg(test)]` opens a region, on purpose: `#[cfg(all(test, unix))]`
/// gates code the same way and is NOT recognised here, so a spawn under one is reported. That is
/// the fail-closed direction — the answer is to come here and decide, not to widen a pattern until
/// something slips under it.
///
/// Reports rather than guesses when a block never closes.
fn cfg_test_regions(
    relative: &str,
    code: &str,
    skeleton: &str,
) -> Result<Vec<(usize, usize)>, String> {
    let sb = skeleton.as_bytes();
    let mut regions = Vec::new();

    for (at, _) in code.match_indices(CFG_TEST) {
        let mut j = at + CFG_TEST.len();

        // Further attributes may sit between the gate and the item it gates:
        // `#[cfg(test)] #[path = "…"] mod support;` is exactly that shape in this tree.
        loop {
            while sb.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            if sb.get(j) != Some(&b'#') || sb.get(j + 1) != Some(&b'[') {
                break;
            }
            let mut depth = 0usize;
            let mut k = j + 1;
            let end = loop {
                let Some(&c) = sb.get(k) else {
                    return Err(format!(
                        "an attribute after `#[cfg(test)]` at {relative}:{} never closes; this \
                         guard cannot tell which code that gate covers.",
                        line_of(code, at)
                    ));
                };
                match c {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            break k + 1;
                        }
                    }
                    _ => {}
                }
                k += 1;
            };
            j = end;
        }

        // A `use` ends at its semicolon and may carry braces (`use a::{b, c};`), which brace
        // matching would read as a block. Neither it nor an `extern crate` gates any code.
        let rest = &skeleton[j..];
        if rest.starts_with("use ") || rest.starts_with("extern crate") {
            continue;
        }

        // The item's own block, if it has one. A `;` first means a declaration — `mod tests;` —
        // which gates a whole FILE rather than a span, and is handled by the allowlist instead.
        let mut k = j;
        let mut open = None;
        while let Some(&c) = sb.get(k) {
            match c {
                b';' => break,
                b'{' => {
                    open = Some(k);
                    break;
                }
                _ => k += 1,
            }
        }
        let Some(open) = open else { continue };

        let mut depth = 0usize;
        let mut m = open;
        let close = loop {
            let Some(&c) = sb.get(m) else {
                return Err(format!(
                    "a `#[cfg(test)]` block opened at {relative}:{} never closes; this guard \
                     cannot tell which code ships in that file.",
                    line_of(code, at)
                ));
            };
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break m + 1;
                    }
                }
                _ => {}
            }
            m += 1;
        };
        regions.push((at, close));
    }

    Ok(regions)
}

fn under_cfg_test(regions: &[(usize, usize)], at: usize) -> bool {
    regions.iter().any(|&(from, to)| at >= from && at < to)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// What a file says about starting a process.

/// One place a file names a way to start a process.
struct Spawn {
    at: usize,
    line: usize,
    spelling: &'static str,
}

fn spawn_sites(code: &str) -> Vec<Spawn> {
    let mut found = Vec::new();
    for spelling in NOTHING_HERE_STARTS_A_PROCESS {
        for (at, _) in code.match_indices(spelling) {
            found.push(Spawn {
                at,
                line: line_of(code, at),
                spelling,
            });
        }
    }
    found.sort_by_key(|s| s.at);
    found
}

/// The program a `Command::new(` at `at` names, or why it is not a fixed one.
///
/// Two shapes are fixed: a string literal, and [`THIS_BINARY`]. Everything else — an identifier, a
/// `format!`, a join, a variable — is refused, because that is the shape a string that came in
/// from outside would have to take to become a command line.
fn program_named_at(code: &str, at: usize) -> Result<String, String> {
    let b = code.as_bytes();
    let mut i = at + "Command::new".len();
    if b.get(i) != Some(&b'(') {
        return Err(
            "`Command::new` is named without being called here, so this guard cannot see \
                    which program it would start"
                .to_string(),
        );
    }
    i += 1;
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }

    if code[i..].starts_with(THIS_BINARY) {
        return Ok(THIS_BINARY.to_string());
    }

    if b.get(i) == Some(&b'"') {
        let mut j = i + 1;
        let mut out = String::new();
        while let Some(&c) = b.get(j) {
            match c {
                b'\\' => {
                    out.push('\\');
                    j += 2;
                }
                b'"' => return Ok(out),
                _ => {
                    out.push(c as char);
                    j += 1;
                }
            }
        }
        return Err("the program's name is a string that never closes".to_string());
    }

    let tail: String = code[i..].chars().take(48).collect();
    Err(format!(
        "the program is built from a value rather than named: `{}`",
        tail.split('\n').next().unwrap_or("").trim()
    ))
}

/// Every place a file reaches for code this scan does not walk.
fn directive_findings(code: &str) -> Vec<(usize, &'static str, String)> {
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        for directive in NO_CODE_FROM_ELSEWHERE {
            // A `cfg_attr` is only a way out when what it carries is a `path`; the rest of the time
            // it is an ordinary conditional attribute, and a rule that tripped on every one of
            // those would be switched off within a week.
            if directive == "cfg_attr" && !line.contains("path") {
                continue;
            }
            if line.contains(directive) {
                found.push((n + 1, directive, line.trim().to_string()));
            }
        }
    }
    found
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The scan.

/// Everything wrong, in one list. Empty means nothing inbound can start a process.
fn scan(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    let members = match workspace_members(root) {
        Ok(m) => m,
        Err(why) => {
            problems.push(why);
            return problems;
        }
    };

    // Every member's sources, plus the one file a redirect pulls into a crate from outside `src/`.
    let mut files: Vec<String> = Vec::new();
    for member in &members {
        let manifest = format!("{member}/Cargo.toml");
        match read_source(root, &manifest) {
            Err(why) => problems.push(why),
            Ok(src) => {
                for section in NO_TARGET_SOMEWHERE_ELSE {
                    if src.contains(section) {
                        problems.push(format!(
                            "{manifest} declares `{section}`, which can point a compiled target at \
                             a directory this scan does not read. Decide here where the walk goes \
                             before that target ships."
                        ));
                    }
                }
            }
        }

        let src_dir = format!("{member}/src");
        let (found, mut walk_problems) = rust_files_under(root, &src_dir);
        problems.append(&mut walk_problems);
        if found.is_empty() {
            problems.push(format!(
                "{src_dir} contributed no source file to this scan. A member that looks empty is a \
                 member this guard did not read, not one that starts nothing."
            ));
        }
        files.extend(found);
    }

    if root.join(THE_ONE_REDIRECT.pulls_in).is_file() {
        files.push(THE_ONE_REDIRECT.pulls_in.to_string());
    } else {
        problems.push(format!(
            "{} is the one file a `#[path]` pulls into a crate from outside `src/`, and it is not \
             there. Either the redirect is stale or the file moved; either way this guard is \
             following a path to nowhere.",
            THE_ONE_REDIRECT.pulls_in
        ));
    }
    files.sort();
    files.dedup();

    // The anti-vacuity half: every file this guard has an opinion about must be IN the walk. A
    // rename, a move out from under `src/`, or a member dropped from the manifest all read as
    // "found nothing" otherwise, which is the failure this whole file exists to refuse.
    let walked: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    for allowed in MAY_START_A_PROCESS {
        if !walked.contains(allowed.file) {
            problems.push(format!(
                "{} is allowed to start a process and this scan never reached it. Its exemption \
                 cannot stand over a file that is not there: {}",
                allowed.file, allowed.why
            ));
        }
    }
    for inbound in THE_INBOUND_PATH {
        if !walked.contains(inbound) {
            problems.push(format!(
                "{inbound} is on the path inbound content takes and this scan never reached it. A \
                 file that has been renamed or moved is not a file that starts nothing."
            ));
        }
    }

    // Read everything once.
    let mut sources: Vec<(String, String, String)> = Vec::new();
    for relative in &files {
        match read_source(root, relative) {
            Err(why) => problems.push(why),
            Ok(src) => match strip(relative, &src) {
                Err(why) => problems.push(why),
                Ok((code, skeleton)) => sources.push((relative.clone(), code, skeleton)),
            },
        }
    }

    let allowed_by_name: std::collections::BTreeMap<&str, &MayStartAProcess> =
        MAY_START_A_PROCESS.iter().map(|a| (a.file, a)).collect();

    for (relative, code, skeleton) in &sources {
        // Code coming in from a file this walk never opens, with the one pinned exception.
        for (line, directive, text) in directive_findings(code) {
            let is_the_one =
                relative == THE_ONE_REDIRECT.in_file && text == THE_ONE_REDIRECT.declaration;
            if is_the_one {
                continue;
            }
            problems.push(format!(
                "{relative}:{line} uses `{directive}`, which brings code into a crate from a file \
                 this scan does not walk. There is exactly one of those in this tree and it is \
                 named in this guard; add code under `src/` instead, or come here and decide."
            ));
        }

        let regions = match cfg_test_regions(relative, code, skeleton) {
            Ok(r) => r,
            Err(why) => {
                problems.push(why);
                continue;
            }
        };

        let sites = spawn_sites(code);

        // Half one: the inbound path. Named files, no way to start anything in the half that ships.
        if THE_INBOUND_PATH.contains(&relative.as_str()) {
            for site in &sites {
                if !under_cfg_test(&regions, site.at) {
                    problems.push(format!(
                        "{relative}:{} names `{}`, and that file is on the path a Telegram message, \
                         a tap and a bridge's frame take. Nothing inbound may reach anything that \
                         starts a process.",
                        site.line, site.spelling
                    ));
                }
            }
        }

        // Half two: the allowlist. Anything else that names a way to start a process is a file
        // whose reason nobody has written down.
        let Some(allowed) = allowed_by_name.get(relative.as_str()) else {
            for site in &sites {
                problems.push(format!(
                    "{relative}:{} names `{}`, and that file is not one of the {} allowed to start \
                     a process. Say here why it may, or do not start one.",
                    site.line,
                    site.spelling,
                    MAY_START_A_PROCESS.len()
                ));
            }
            continue;
        };

        // Every spawn names a fixed program, and exactly the declared ones are found.
        let declared: BTreeSet<&str> = allowed.programs.iter().copied().collect();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for site in sites.iter().filter(|s| s.spelling == "Command::new") {
            match program_named_at(code, site.at) {
                Ok(program) => {
                    if !declared.contains(program.as_str()) {
                        problems.push(format!(
                            "{relative}:{} starts `{program}`, which is not one of the programs \
                             this file is allowed to name ({}). Adding one is a decision about \
                             what this product does.",
                            site.line,
                            allowed.programs.join(", ")
                        ));
                    }
                    seen.insert(program);
                }
                Err(why) => problems.push(format!(
                    "{relative}:{} — {why}. A program this guard cannot read as a fixed name is \
                     the shape a string from the wire would have to take to become a command line, \
                     so it is refused even here.",
                    site.line
                )),
            }
        }
        for program in &declared {
            if !seen.contains(*program) {
                problems.push(format!(
                    "{relative} is allowed to start `{program}` and no longer does. An exemption \
                     nobody needs must be taken out rather than left standing over whatever the \
                     file becomes next: {}",
                    allowed.why
                ));
            }
        }

        // And what the file's own gating is worth.
        match allowed.ships {
            Ships::Yes {
                sites: expected,
                argv_heads,
            } => {
                let shipped: Vec<&Spawn> = sites
                    .iter()
                    .filter(|s| s.spelling == "Command::new" && !under_cfg_test(&regions, s.at))
                    .collect();
                if shipped.len() != expected {
                    problems.push(format!(
                        "{relative} starts {} process(es) in the half that ships, and {expected} \
                         is what this guard was told to expect{}. Adding one is a deliberate act; \
                         this is where it is decided. The reason the ones there are allowed: {}",
                        shipped.len(),
                        shipped
                            .iter()
                            .map(|s| format!(" (line {})", s.line))
                            .collect::<Vec<_>>()
                            .join(""),
                        allowed.why
                    ));
                }

                // Blank the test blocks and look for each subcommand in what is left, so a shipped
                // site swapped for another kind of git call is caught rather than merely counted.
                let mut shipped_code = code.clone().into_bytes();
                for &(from, to) in &regions {
                    blank(&mut shipped_code, from, to);
                }
                let shipped_code = String::from_utf8_lossy(&shipped_code).into_owned();
                for head in argv_heads {
                    if !shipped_code.contains(head) {
                        problems.push(format!(
                            "{relative} no longer passes `{head}` in the half that ships. Each \
                             shipped site is named here by the question it asks git, so a site \
                             that starts asking a different one is a change somebody decided on."
                        ));
                    }
                }
            }
            Ships::OnlyUnderCfgTest => {
                if regions.is_empty() {
                    problems.push(format!(
                        "{relative} is excused because everything that starts a process in it sits \
                         under `#[cfg(test)]`, and there is no such block in it any more. The \
                         exemption has to be re-decided rather than quietly applied to whatever \
                         the file holds now."
                    ));
                }
                for site in &sites {
                    if !under_cfg_test(&regions, site.at) {
                        problems.push(format!(
                            "{relative}:{} names `{}` outside `#[cfg(test)]`, so it is in the \
                             binary the operator runs. That file is excused only for what its own \
                             tests do: {}",
                            site.line, site.spelling, allowed.why
                        ));
                    }
                }
            }
            Ships::WholeFileGatedAt {
                declared_in,
                declaration,
            } => {
                let held = sources.iter().find(|(f, _, _)| f == declared_in);
                match held {
                    None => problems.push(format!(
                        "{relative} is excused because {declared_in} declares it behind \
                         `#[cfg(test)]`, and this scan never read {declared_in}. The gate is the \
                         whole exemption; a gate it cannot see is one it must not assume."
                    )),
                    Some((_, code, _)) => {
                        let flat = code.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !flat.contains(declaration) {
                            problems.push(format!(
                                "{declared_in} no longer says `{declaration}`, and that line is the \
                                 only thing keeping {relative} out of the shipped binary. It holds \
                                 {}: {}",
                                allowed.programs.join(", "),
                                allowed.why
                            ));
                        }
                    }
                }
            }
        }
    }

    problems.sort();
    problems.dedup();
    problems
}

/// How the scan reads when it fails: one problem per line, so the failure is the to-do list.
fn report(problems: &[String]) -> String {
    problems
        .iter()
        .map(|p| format!("  - {p}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The property.

#[test]
fn nothing_the_bot_reads_can_reach_a_process() {
    let problems = scan(&workspace_root());
    assert!(
        problems.is_empty(),
        "a message, a tap or a frame could reach something that starts a process, in {} place(s):\n{}",
        problems.len(),
        report(&problems)
    );
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The guard's own guards. A scanner nobody attacked is a scanner that reports green.

/// A tree shaped like this repo's, in which nothing inbound can start a process.
///
/// Every allowed file is written with what it is allowed for, because an allowlist entry whose
/// file no longer spawns is itself a failure and a rig that left them out would never exercise
/// that. Every inbound-path file is written clean.
fn a_tree_that_starts_only_what_it_should() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path();

    write_at(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/herdr-client\", \"crates/herdr-tg\", \
         \"crates/hub-proto\"]\n",
    );
    for member in ["crates/herdr-client", "crates/herdr-tg", "crates/hub-proto"] {
        write_at(root, &format!("{member}/Cargo.toml"), "[package]\n");
    }

    write_at(root, "crates/hub-proto/src/lib.rs", "pub struct Frame;\n");
    write_at(
        root,
        "crates/herdr-client/src/lib.rs",
        "#[cfg(test)]\n#[path = \"../tests/support/mod.rs\"]\nmod support;\n",
    );
    write_at(
        root,
        "crates/herdr-client/tests/support/mod.rs",
        "pub fn a_stand_in() {}\n",
    );

    for inbound in THE_INBOUND_PATH {
        write_at(root, inbound, "pub fn nothing_here_starts_anything() {}\n");
    }

    // The hub declares its test module behind the gate, and that module is where the bun bridge,
    // the git repos, the fifo and the dead pid come from.
    write_at(
        root,
        "crates/herdr-tg/src/hub.rs",
        "pub struct Claim;\n\n#[cfg(test)]\nmod tests;\n",
    );
    write_at(
        root,
        "crates/herdr-tg/src/hub/tests.rs",
        "fn f() {\n    std::process::Command::new(\"/bin/true\");\n    \
         tokio::process::Command::new(\"bun\");\n    \
         std::process::Command::new(\"git\");\n    \
         std::process::Command::new(\"mkfifo\");\n    \
         std::process::Command::new(\"sh\");\n    let _ = std::process::Stdio::null();\n}\n",
    );

    // Enrolment: three that ship, each with the question it asks git, and two in its own tests.
    write_at(
        root,
        "crates/herdr-tg/src/cmd/enroll.rs",
        "fn ask() {\n    std::process::Command::new(\"git\").args([\"rev-parse\", \
         \"--show-toplevel\"]);\n    std::process::Command::new(\"git\").args([\"check-ignore\", \
         \"-q\"]);\n    std::process::Command::new(\"git\").args([\"ls-files\", \
         \"--error-unmatch\"]);\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        \
         std::process::Command::new(\"git\").args([\"init\", \"-q\"]);\n        \
         std::process::Command::new(\"git\").args([\"commit\", \"-qm\", \"{}\"]);\n    }\n}\n",
    );

    // The two files whose only spawns are under their own gate. Overwrites the clean stand-ins
    // written above, which is the point: they are on the inbound path AND allowed, under `test`.
    write_at(
        root,
        "crates/herdr-tg/src/presence.rs",
        "pub fn vouched_for() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        \
         std::process::Command::new(\"/bin/true\");\n    }\n}\n",
    );
    write_at(
        root,
        "crates/herdr-tg/src/summarize.rs",
        "pub fn gist() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        \
         std::process::Command::new(std::env::current_exe().expect(\"the test binary\"));\n    }\n}\n",
    );

    dir
}

fn write_at(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("a place to put the file");
    std::fs::write(&path, body).unwrap_or_else(|e| panic!("write {relative}: {e}"));
}

#[test]
fn a_tree_in_which_nothing_inbound_starts_a_process_is_reported_clean() {
    let dir = a_tree_that_starts_only_what_it_should();
    let problems = scan(dir.path());
    assert!(
        problems.is_empty(),
        "the rig this guard's own tests plant defects in was itself reported as broken, so every \
         one of them would pass for the wrong reason:\n{}",
        report(&problems)
    );
}

#[test]
fn a_workspace_this_guard_cannot_read_fails_the_scan_instead_of_finding_nothing() {
    let nowhere = tempfile::tempdir().expect("a temp dir");
    let problems = scan(nowhere.path());
    assert!(
        !problems.is_empty(),
        "a scan of an empty directory found nothing to say, which is what 'clean' looks like"
    );
    assert!(
        problems.iter().any(|p| p.contains("Cargo.toml")),
        "a scan that could not read the workspace manifest reported this instead:\n{}",
        report(&problems)
    );
}

#[test]
fn a_member_that_contributes_no_source_file_fails_rather_than_being_skipped() {
    let dir = a_tree_that_starts_only_what_it_should();
    std::fs::remove_dir_all(dir.path().join("crates/hub-proto/src"))
        .expect("take its sources away");
    let problems = scan(dir.path());
    assert!(
        problems.iter().any(
            |p| p.contains("crates/hub-proto/src") && p.contains("did not read")
                || p.contains("crates/hub-proto/src") && p.contains("not a directory")
        ),
        "a member whose sources are gone was walked past in silence:\n{}",
        report(&problems)
    );
}

#[test]
fn a_spawn_planted_in_the_hub_is_reported_rather_than_walked_past() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/hub.rs",
        "pub struct Claim;\n\nfn start_it() {\n    std::process::Command::new(\"systemctl\");\n}\n\
         \n#[cfg(test)]\nmod tests;\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/hub.rs:4") && p.contains("Command::new")),
        "a process started from the hub — the file every frame from a bridge flows through — was \
         not reported:\n{}",
        report(&problems)
    );
}

#[test]
fn a_spawn_planted_in_a_module_beside_the_hub_is_found_there_too() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/hub/door.rs",
        "fn open() {\n    std::process::Command::new(\"socat\");\n}\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/hub/door.rs:2")),
        "splitting the hub into modules is the ordinary change that would take this guard's sight \
         away, and a spawn in a new one was not reported:\n{}",
        report(&problems)
    );
}

#[test]
fn a_program_built_from_a_value_is_refused_even_in_a_file_that_may_spawn() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/cmd/enroll.rs",
        "fn ask(chosen: &str) {\n    std::process::Command::new(chosen).args([\"rev-parse\"]);\n    \
         std::process::Command::new(\"git\").args([\"check-ignore\", \"-q\"]);\n    \
         std::process::Command::new(\"git\").args([\"ls-files\", \"--error-unmatch\"]);\n}\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/cmd/enroll.rs:2")
                && p.contains("built from a value")),
        "a program assembled from a value — the only shape a string from the wire could reach a \
         command line by — passed inside a file that is allowed to start `git`:\n{}",
        report(&problems)
    );
}

#[test]
fn an_allowed_file_that_has_been_renamed_fails_rather_than_scanning_nothing() {
    let dir = a_tree_that_starts_only_what_it_should();
    let src = dir.path().join("crates/herdr-tg/src/cmd/enroll.rs");
    let moved = dir.path().join("crates/herdr-tg/src/cmd/enrolment.rs");
    std::fs::rename(&src, &moved).expect("rename the allowed file");
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/cmd/enroll.rs")
                && p.contains("never reached it")),
        "the file whose three git calls this guard is written about was renamed and the scan \
         carried on:\n{}",
        report(&problems)
    );
    // And the file it became is not silently inheriting the exemption.
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/cmd/enrolment.rs")),
        "the renamed file kept the old one's licence to start a process:\n{}",
        report(&problems)
    );
}

#[test]
fn an_inbound_file_that_has_been_renamed_fails_rather_than_going_unwatched() {
    let dir = a_tree_that_starts_only_what_it_should();
    std::fs::rename(
        dir.path().join("crates/herdr-tg/src/queue.rs"),
        dir.path().join("crates/herdr-tg/src/pacer.rs"),
    )
    .expect("rename the pacer");
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/queue.rs")),
        "a file on the inbound path was renamed and this guard went on believing it had read it:\n{}",
        report(&problems)
    );
}

#[test]
fn a_fourth_shipped_call_site_in_enrolment_is_reported_rather_than_counted_in() {
    let dir = a_tree_that_starts_only_what_it_should();
    let path = dir.path().join("crates/herdr-tg/src/cmd/enroll.rs");
    let mut src = std::fs::read_to_string(&path).expect("read the stand-in");
    src.push_str("fn one_more() { std::process::Command::new(\"git\").args([\"fetch\"]); }\n");
    std::fs::write(&path, src).expect("plant a fourth shipped site");
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/cmd/enroll.rs")
                && p.contains("in the half that ships")),
        "a fourth git call in the shipped half of enrolment was absorbed rather than reported:\n{}",
        report(&problems)
    );
}

#[test]
fn a_spawn_that_leaves_its_test_block_stops_being_excused() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/presence.rs",
        "pub fn vouched_for() {\n    std::process::Command::new(\"/bin/true\");\n}\n\n\
         #[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        \
         std::process::Command::new(\"/bin/true\");\n    }\n}\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/presence.rs:2")),
        "the same program, moved out of the test block and into the shipped binary, was still \
         excused by the entry that only ever covered the test:\n{}",
        report(&problems)
    );
}

#[test]
fn a_test_module_that_stops_being_gated_takes_its_programs_into_the_binary_and_is_said_so() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/hub.rs",
        "pub struct Claim;\n\nmod tests;\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/hub.rs") && p.contains("mod tests;")),
        "dropping `#[cfg(test)]` from the hub's test module puts bun, git, sh and mkfifo in the \
         shipped binary, and the scan said nothing:\n{}",
        report(&problems)
    );
}

#[test]
fn an_exemption_whose_reason_has_gone_is_refused_rather_than_left_standing() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/src/presence.rs",
        "pub fn vouched_for() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/src/presence.rs")
                && p.contains("no longer does")),
        "a file kept its licence to start a process after it had stopped needing one, which is how \
         an exemption comes to cover whatever the file becomes next:\n{}",
        report(&problems)
    );
}

#[test]
fn code_pulled_into_a_crate_from_a_file_this_scan_does_not_walk_is_refused() {
    for directive in [
        "include!(\"../elsewhere.rs\");",
        "#[path = \"../elsewhere.rs\"] mod elsewhere;",
        "#[cfg_attr(unix, path = \"../elsewhere.rs\")] mod elsewhere;",
    ] {
        let dir = a_tree_that_starts_only_what_it_should();
        let path = dir.path().join("crates/herdr-tg/src/bot.rs");
        let mut src = std::fs::read_to_string(&path).expect("read the stand-in bot");
        src.push_str(directive);
        src.push('\n');
        std::fs::write(&path, src).expect("plant the directive");

        let problems = scan(dir.path());
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("crates/herdr-tg/src/bot.rs")),
            "`{directive}` brings code into the crate from a file this scan never opens, and the \
             scan said nothing:\n{}",
            report(&problems)
        );
    }
}

#[test]
fn the_one_redirect_this_tree_really_has_is_followed_and_not_merely_forgiven() {
    let dir = a_tree_that_starts_only_what_it_should();
    // The file the redirect names is real code in a real crate, so a spawn in it counts.
    write_at(
        dir.path(),
        "crates/herdr-client/tests/support/mod.rs",
        "pub fn a_stand_in() {\n    std::process::Command::new(\"herdr\");\n}\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-client/tests/support/mod.rs:2")),
        "the one file a `#[path]` mounts into a crate was forgiven rather than followed:\n{}",
        report(&problems)
    );
}

#[test]
fn a_symlinked_directory_is_refused_rather_than_walked_past_in_silence() {
    let dir = a_tree_that_starts_only_what_it_should();
    let elsewhere = dir.path().join("outside");
    std::fs::create_dir_all(&elsewhere).expect("a directory outside the crate");
    std::fs::write(
        elsewhere.join("door.rs"),
        "fn open() { std::process::Command::new(\"socat\"); }\n",
    )
    .expect("a spawn in it");
    std::os::unix::fs::symlink(elsewhere, dir.path().join("crates/herdr-tg/src/hub/parts"))
        .expect("link it in");

    let problems = scan(dir.path());
    assert!(
        problems.iter().any(|p| p.contains("symbolic link")),
        "a directory linked in under `src/` was skipped without a word; the guard reported on a \
         tree it had not finished looking at:\n{}",
        report(&problems)
    );
}

#[test]
fn a_target_pointed_somewhere_this_scan_does_not_read_is_refused() {
    let dir = a_tree_that_starts_only_what_it_should();
    write_at(
        dir.path(),
        "crates/herdr-tg/Cargo.toml",
        "[package]\n\n[[bin]]\nname = \"herdr-tg\"\npath = \"launcher/main.rs\"\n",
    );
    let problems = scan(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("crates/herdr-tg/Cargo.toml") && p.contains("[[bin]]")),
        "a shipped program was pointed at a directory this walk never opens, and the scan said \
         nothing:\n{}",
        report(&problems)
    );
}

#[test]
fn every_spelling_this_guard_denies_is_one_it_can_actually_find() {
    // Six of these match nothing in the tree today. A rule for tomorrow written with a typo in it
    // is a rule that will be green on the day it matters, which is how a list rots into words that
    // cannot match.
    for spelling in NOTHING_HERE_STARTS_A_PROCESS {
        let planted = format!("fn f() {{ let x = {spelling}; }}\n");
        let (code, _) = strip("planted.rs", &planted).expect("the plant is well formed");
        assert!(
            spawn_sites(&code).iter().any(|s| s.spelling == spelling),
            "`{spelling}` is in the denied list and planting it found nothing, so this guard has \
             been scanning for a word that cannot match"
        );
    }
}

#[test]
fn every_program_the_allowlist_names_is_one_the_real_file_still_starts() {
    // The other half of the anti-vacuity rule, run against the real tree rather than a rig: an
    // allowlist entry is only worth anything while the file it excuses is still the file it was
    // written about.
    let root = workspace_root();
    for allowed in MAY_START_A_PROCESS {
        let src = read_source(&root, allowed.file).expect("an allowed file is readable");
        let (code, _) = strip(allowed.file, &src).expect("an allowed file is well formed");
        let found: BTreeSet<String> = spawn_sites(&code)
            .iter()
            .filter(|s| s.spelling == "Command::new")
            .filter_map(|s| program_named_at(&code, s.at).ok())
            .collect();
        for program in allowed.programs {
            assert!(
                found.contains(*program),
                "{} is allowed to start `{program}` and does not; it starts {found:?}",
                allowed.file
            );
        }
    }
}

#[test]
fn a_program_in_a_comment_is_prose_but_the_same_program_in_a_string_is_code() {
    let commented = concat!(
        "// this binary starts nothing: no Command::new, no Stdio, no current_exe\n",
        "/* std::process::Command::new(\"git\") is what enrolment does, not this */\n",
        "/// `tokio::process` is a dev-only feature here\n",
        "fn nothing() {}\n"
    );
    let (code, _) = strip("commented.rs", commented).expect("well formed");
    assert!(
        spawn_sites(&code).is_empty(),
        "a comment explaining WHY this binary starts nothing was read as it starting something"
    );

    let stringed = "fn f() { std::process::Command::new(\"git\"); }\n";
    let (code, _) = strip("stringed.rs", stringed).expect("well formed");
    assert!(
        !spawn_sites(&code).is_empty(),
        "a program named in a string literal was blanked along with the comments"
    );
}

#[test]
fn a_brace_inside_a_string_does_not_move_the_end_of_a_test_module() {
    // The failure this shape causes is silent and total: an unbalanced `{` in a format string
    // swallows the rest of the file into the `#[cfg(test)]` region, and every spawn after it reads
    // as test-only. Brace matching runs over a copy with string bodies blanked for exactly this.
    let src = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() { let _ = \"a lonely { brace\"; }\n",
        "}\n",
        "fn ships() { std::process::Command::new(\"systemctl\"); }\n"
    );
    let (code, skeleton) = strip("braced.rs", src).expect("well formed");
    let regions = cfg_test_regions("braced.rs", &code, &skeleton).expect("the block closes");
    let site = spawn_sites(&code)
        .into_iter()
        .find(|s| s.spelling == "Command::new")
        .expect("the shipped spawn is found");
    assert_eq!(site.line, 5, "the shipped spawn is on the last line");
    assert!(
        !under_cfg_test(&regions, site.at),
        "a `{{` inside a string extended the test module over the rest of the file, and a spawn in \
         the shipped binary read as test-only"
    );
}

#[test]
fn a_gate_this_guard_does_not_recognise_leaves_a_spawn_reported_rather_than_excused() {
    // `#[cfg(all(test, unix))]` gates code just as well and is deliberately NOT read as a test
    // block. Fail closed: the answer is to come to this file and decide, not to widen a pattern.
    let src = concat!(
        "#[cfg(all(test, unix))]\n",
        "mod tests {\n",
        "    fn t() { std::process::Command::new(\"bun\"); }\n",
        "}\n"
    );
    let (code, skeleton) = strip("gated.rs", src).expect("well formed");
    let regions = cfg_test_regions("gated.rs", &code, &skeleton).expect("nothing to close");
    let site = spawn_sites(&code)
        .into_iter()
        .find(|s| s.spelling == "Command::new")
        .expect("the spawn is found");
    assert!(
        !under_cfg_test(&regions, site.at),
        "a gate this guard has never been told about silently excused a spawn"
    );
}

#[test]
fn a_use_that_carries_braces_is_not_read_as_a_test_module() {
    // `#[cfg(test)] use a::{b, c};` gates an import and no code at all. Reading its braces as a
    // block would put whatever followed inside a region that does not exist.
    let src = concat!(
        "#[cfg(test)]\n",
        "use super::{one, two};\n",
        "fn ships() { std::process::Command::new(\"systemctl\"); }\n"
    );
    let (code, skeleton) = strip("used.rs", src).expect("well formed");
    let regions = cfg_test_regions("used.rs", &code, &skeleton).expect("nothing to close");
    let site = spawn_sites(&code)
        .into_iter()
        .find(|s| s.spelling == "Command::new")
        .expect("the spawn is found");
    assert!(
        !under_cfg_test(&regions, site.at),
        "an import's braces were read as a test module, and a shipped spawn fell inside it"
    );
}

#[test]
fn an_unterminated_comment_is_reported_rather_than_scanned_around() {
    let err = strip("torn.rs", "/* it opens\nfn f() { }\n").expect_err("an unclosed comment");
    assert!(
        err.contains("torn.rs:1") && err.contains("never closes"),
        "a file this guard cannot tell code from prose in was scanned anyway: {err}"
    );
}

#[test]
fn the_two_shapes_of_a_fixed_program_are_read_and_nothing_else_is() {
    for (src, expected) in [
        ("std::process::Command::new(\"git\")", "git"),
        ("Command::new(\n    \"/bin/true\",\n)", "/bin/true"),
        (
            "std::process::Command::new(std::env::current_exe().expect(\"the test binary\"))",
            THIS_BINARY,
        ),
    ] {
        let (code, _) = strip("p.rs", src).expect("well formed");
        let at = code.find("Command::new").expect("the constructor is there");
        assert_eq!(
            program_named_at(&code, at).as_deref(),
            Ok(expected),
            "a fixed program was not read as one: {src}"
        );
    }

    for src in [
        "Command::new(chosen)",
        "Command::new(format!(\"{}\", from_the_wire))",
        "Command::new(&path)",
        "Command::new(std::env::var(\"SHELL\").unwrap())",
    ] {
        let (code, _) = strip("p.rs", src).expect("well formed");
        let at = code.find("Command::new").expect("the constructor is there");
        assert!(
            program_named_at(&code, at).is_err(),
            "a program built from a value was read as a fixed one: {src}"
        );
    }
}

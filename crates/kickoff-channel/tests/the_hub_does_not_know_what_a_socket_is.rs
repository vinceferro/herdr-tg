//! **The transport seam, asserted rather than described in a doc comment.**
//!
//! `hub.rs` is the semantics layer: who a connection belongs to, what a claim is, which ask is
//! open, which topic a line goes to. `transport.rs` is the bytes: a listener, an accepted
//! connection, and who the kernel says is on the other end of it. Before this guard the two were
//! one file — the hub bound its own `UnixListener`, read `SO_PEERCRED` off the stream, and derived
//! `/run/user/<uid>/kickoff/hub.sock` — and the cost was not tidiness. It was that every test of
//! the semantics had to create a real socket file, and that the day a second transport is wanted
//! (a gateway, per `docs/CAPABILITIES.md` OPEN 4) the change lands in the 5000-line file that also
//! decides who may answer a question.
//!
//! A seam that only a module doc claims is one refactor away from being false, so it is asserted
//! here, in both directions:
//!
//! 1. **`hub.rs` may not NAME a socket** — nor may any module beside it under `src/hub/`, save the
//!    one file that drives the real thing over a real socket on purpose. Not the two tokio types,
//!    not the peer-credential call, not the path, not `into_split`. Naming one is how the seam
//!    leaks back: an "I'll just reach for the stream here" that nothing objects to.
//! 2. **`transport.rs` may not NAME a claim, an ask, a topic or a registry.** The direction that
//!    is easy to forget. A transport that learns what a claim is has taken the hub's job, and the
//!    second transport then has to reimplement it.
//!
//! # This guard FAILS when it cannot look
//!
//! The sibling guard in `crates/herdr-client/tests/no_live_write_call_site.rs` was walked past six
//! times, and nearly every one ended the same way: something it was asked to resolve did not
//! resolve, and the code quietly carried on. So every lookup here is loud:
//!
//! * A file it cannot read is a FAILURE, never "nothing found". A missing `transport.rs` does not
//!   mean the transport names no claim; it means the guard has no idea.
//! * Each file must still hold the ANCHORS it is named for — `serve_connection` and `Claim` in the
//!   hub, `ConnectionIdentity` and `LocalSocket` in the transport — and they are looked for in the
//!   COMMENT-STRIPPED text, so a stripper that ate the whole file cannot pass as a clean scan.
//! * The stripper itself fails on an unterminated comment or string rather than guessing.
//! * [`the_hub_as_it_stood_before_the_transport_moved_out_is_still_caught_by_this_scan`] runs the
//!   scan over the hub as it was at the commit before the seam, read out of git, and requires each
//!   spelling that was there to be reported. A spelling in the list below that matches nothing —
//!   a typo, a rename, a word that was never in the file — cannot leave this guard vacuously green.
//!
//! # Comments are prose; string literals are code
//!
//! The scan runs over the source with COMMENTS BLANKED OUT and string literals left in place.
//! Comments explain WHY in this repo, and a comment in `hub.rs` saying "the transport reads
//! SO_PEERCRED for us" is the code being well explained, not the seam leaking. A string literal is
//! different: `"hub.sock"` in the hub IS the hub knowing the path. The one word that is deliberately
//! NOT forbidden is the plain English "socket" — the hub's journal lines say true things about the
//! connection ending, and it would be a jargon-free sentence killed for a rule's convenience.
//!
//! It reads the two files, everything under the hub's own module directory, and one blob out of
//! this repository's git object store. It opens no socket.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The semantics layer. Everything about who may do what.
const HUB: &str = "crates/kickoff-channel/src/hub.rs";

/// The bytes layer. A listener, a connection, and who is on the other end of it.
const TRANSPORT: &str = "crates/kickoff-channel/src/transport.rs";

/// The transport's own module directory, once it has one.
///
/// There is no such directory today — the transport is one file — so unlike [`HUB_DIR`] its absence
/// is not a problem. It is scanned because a second transport is the change this file was split out
/// for, and the day it arrives is the day the transport half of this guard would otherwise go blind.
const TRANSPORT_DIR: &str = "crates/kickoff-channel/src/transport";

/// The last commit in which the hub opened its own socket.
///
/// Read back out of git by
/// [`the_hub_as_it_stood_before_the_transport_moved_out_is_still_caught_by_this_scan`], which is
/// what keeps [`NOT_IN_THE_HUB`] from becoming a list of words that match nothing.
const BEFORE_THE_SEAM: &str = "13ea2d4";

/// Where the hub lived at [`BEFORE_THE_SEAM`], which is NOT where it lives now.
///
/// The crate directory was renamed `herdr-tg` -> `kickoff-channel` long after that commit, and
/// `git show <commit>:<path>` resolves the path inside that commit's own tree. Asking for today's
/// path there fails with "exists on disk, but not in <commit>" — a guard that reads nothing, which
/// is the one outcome [`hub_at`] refuses to let pass as "I looked". A rename tomorrow must add the
/// next spelling here rather than edit this one: history does not move with the working tree.
const HUB_BEFORE_THE_SEAM: &str = "crates/herdr-tg/src/hub.rs";

/// Spellings that mean "this file knows it is talking over a Unix socket".
///
/// Matched as plain substrings of the comment-free source, because each is a specific type, path or
/// function spelling rather than an English word. The families:
///
/// * `UnixStream` / `UnixListener` — the tokio types. Holding one is holding the transport.
/// * `SO_PEERCRED` / `socket_peercred` / `peer_cred` / `PeerCred` — asking the kernel who is there.
///   The hub is handed the answer (`ConnectionIdentity`); it does not ask the socket itself.
/// * `into_split` — `UnixStream`'s own halving. The seam splits a `ByteStream` instead, and a hub
///   that still reaches for `into_split` is a hub that still has a stream.
/// * `socket_path` / `hub.sock` / `/run/user` / `AF_UNIX` — the address. A second transport has a
///   different one, and the hub must not be the file that has to change.
const NOT_IN_THE_HUB: [&str; 11] = [
    "UnixStream",
    "UnixListener",
    "SO_PEERCRED",
    "socket_peercred",
    "peer_cred",
    "PeerCred",
    "into_split",
    "socket_path",
    "hub.sock",
    "/run/user",
    "AF_UNIX",
];

/// Words that mean "this file knows what the hub decides".
///
/// Matched as whole WORDS inside identifiers — `AskId`, `ask_id` and `asks` are all the word "ask";
/// `task` and `asked` are not. A substring rule would have failed on `tokio::task` the first time
/// the transport spawned anything, and a guard that cries wolf gets deleted.
const NOT_IN_THE_TRANSPORT: [&str; 8] = [
    "claim",
    "claims",
    "ask",
    "asks",
    "topic",
    "topics",
    "registry",
    "registries",
];

/// Ways to put code IN the hub without putting a file in the hub's directory.
///
/// `include!("…")` splices a file straight into `hub.rs`'s own module; `#[path = "…"]` mounts a
/// module from anywhere on disk, and a `cfg_attr` can carry that conditionally. Either of them puts
/// a `UnixListener` in the shipped binary, inside the hub, with a scan that walks only [`HUB`] and
/// [`HUB_DIR`] reporting green — and two of the six times the sibling guard in
/// `crates/herdr-client/tests/no_live_write_call_site.rs` was walked past were exactly these.
///
/// That guard answered by growing a resolver. This one answers by refusing the directives outright:
/// nothing under the hub has ever needed one, so the rule costs nothing, and unlike a resolver it
/// cannot be evaded by a path the guard fails to follow. The day the hub genuinely needs one, this
/// list is the place the decision gets made rather than the place it gets missed.
const NO_CODE_FROM_ELSEWHERE: [&str; 3] = ["include!", "#[path", "cfg_attr"];

/// Proof `hub.rs` is still the semantics layer and not a file that was gutted or renamed under us.
///
/// `async fn serve_connection(` is pinned by the slice's own compatibility note; `struct Claim {` is
/// the thing the transport is forbidden to know about, so its absence would make half this guard
/// meaningless.
const HUB_ANCHORS: [&str; 2] = ["async fn serve_connection(", "struct Claim {"];

/// Proof `transport.rs` is the transport: who is on the other end, and the local door they came in
/// by. Both names come from the seam's design; if they change, this guard wants to be told.
const TRANSPORT_ANCHORS: [&str; 2] = ["ConnectionIdentity", "LocalSocket"];

/// The hub's own module directory. Everything under it is the hub, and is scanned like `hub.rs`.
///
/// Splitting a 5000-line file into modules is an ordinary, likely change, and a guard that only
/// ever looked at `hub.rs` would go blind on the commit that did it — a `hub/socket.rs` holding a
/// `UnixListener` with the suite green. Deny by default: the whole directory, minus one named file.
const HUB_DIR: &str = "crates/kickoff-channel/src/hub";

/// The one file under [`HUB_DIR`] the socket rule does not apply to, and why.
///
/// The hub's test module drives the real thing over a real socket on purpose — the pre-pong hold
/// and the 64-frame backlog depend on kernel backpressure, and a duplex in memory does not have it.
/// It is asserted to EXIST, so moving those tests elsewhere goes red here rather than silently
/// turning this exemption into a licence for whatever file takes the name.
const HUB_DIR_EXEMPT: &str = "crates/kickoff-channel/src/hub/tests.rs";

/// The workspace root: `crates/kickoff-channel/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

/// Read one of the two files, or say why the guard is blind.
///
/// The `Err` is not a warning to log and carry on with: every caller turns it into a failing scan.
/// A file this guard cannot open tells it nothing about what that file names.
fn read_source(root: &Path, relative: &str) -> Result<String, String> {
    match std::fs::read_to_string(root.join(relative)) {
        Ok(text) => Ok(text),
        Err(e) => Err(format!(
            "{relative} could not be read ({e}). This guard proves nothing about a file it cannot \
             open, so it fails instead of reporting a clean scan."
        )),
    }
}

/// Every `.rs` file under [`HUB_DIR`] except the exempt one, workspace-relative and sorted.
///
/// **Every failure to look is returned as a problem**, never as an empty list: an unreadable
/// directory, an entry whose kind cannot be determined, or a missing exemption all mean the guard
/// does not know what is in there.
fn hub_module_files(root: &Path) -> (Vec<String>, Vec<String>) {
    let (files, mut problems) = rust_files_under(root, HUB_DIR, false);

    if !root.join(HUB_DIR_EXEMPT).is_file() {
        problems.push(format!(
            "{HUB_DIR_EXEMPT} is not there. The socket-driven tests live in it and that is why it is \
             excused from this rule; if they moved, the exemption has to be re-decided rather than \
             quietly applied to whatever holds the name next."
        ));
    }

    let files = files.into_iter().filter(|f| f != HUB_DIR_EXEMPT).collect();
    (files, problems)
}

/// Every `.rs` file under one module directory, workspace-relative and sorted, with every failure
/// to look returned as a problem rather than as an empty list.
///
/// `optional` is for a directory that need not exist yet: `src/transport/` is what the transport
/// becomes when the gateway splits it, and until then there is nothing to walk. It is not a way to
/// go blind — the FILE each directory belongs to is read and anchored separately — and a directory
/// that exists but cannot be listed is still a problem either way.
fn rust_files_under(root: &Path, dir_relative: &str, optional: bool) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();

    let top = root.join(dir_relative);
    if optional && !top.exists() {
        return (files, problems);
    }

    let mut stack = vec![top];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                problems.push(format!(
                    "{} could not be listed ({e}). Everything under it is the hub, and a guard that \
                     cannot see in there cannot say the hub names no socket.",
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
                Ok(_) if path.extension().is_some_and(|e| e == "rs") => {
                    files.push(
                        path.strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
                // `file_type()` is `lstat`-based, so a symlink to a directory answers neither
                // `is_dir()` nor `.rs` and fell through the arm below in silence — a directory
                // shaped like something to skip, which is the class of mistake that walked past the
                // sibling guard four times. Refused rather than followed: a link leads out of the
                // tree this rule is written about, and this guard would then be scanning somewhere
                // else without saying so.
                Ok(t) if t.is_symlink() => problems.push(format!(
                    "{} is a symbolic link, and this guard will not walk through one. Put the file \
                     under {dir_relative} itself, or leave it out.",
                    path.display()
                )),
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

/// The source with every comment blanked to spaces, byte-for-byte the same length so line numbers
/// still line up, and string literals left exactly where they were.
///
/// **Panics on an unterminated comment or string**, which in a file that compiles cannot happen —
/// and if it somehow does, the guard has lost track of where the code is and must not pretend
/// otherwise.
fn code_only(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let opened_at = line_of(src, i);
                let mut depth = 0usize;
                loop {
                    if i >= b.len() {
                        panic!(
                            "a block comment opened at line {opened_at} never closes; this guard \
                             cannot tell code from prose in that file"
                        );
                    }
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        out[i] = b' ';
                        out[i + 1] = b' ';
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        out[i] = b' ';
                        out[i + 1] = b' ';
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if b[i] != b'\n' {
                            out[i] = b' ';
                        }
                        i += 1;
                    }
                }
            }
            b'"' => i = end_of_string(src, i),
            // `r"…"`, `r#"…"#`, `b"…"`, `br#"…"#` — a raw string swallows quotes and backslashes,
            // so reading one as an ordinary string would leave the scanner inside a span that never
            // closes where the code thinks it does.
            b'r' | b'b' if !is_ident_byte(i.checked_sub(1).map(|p| b[p])) => {
                match raw_string_len(b, i) {
                    Some(n) => i += n,
                    None => i += 1,
                }
            }
            // A char literal can hold a quote (`'"'`) or a slash (`'/'`); a lifetime (`'a`) looks
            // almost the same and holds nothing. Getting this wrong opens a string span that eats
            // the rest of the file, which is exactly how a scanner goes quietly blind.
            b'\'' => match char_literal_len(src, i) {
                Some(n) => i += n,
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking comments never splits a character")
}

/// Is the byte before a `r`/`b` part of an identifier? `for` and `char` end in those letters, and a
/// raw-string prefix is only a prefix when nothing is glued to its left.
fn is_ident_byte(b: Option<u8>) -> bool {
    matches!(b, Some(c) if c.is_ascii_alphanumeric() || c == b'_')
}

/// 1-based line number of a byte offset, for a failure that names a place.
fn line_of(src: &str, at: usize) -> usize {
    src[..at].bytes().filter(|&c| c == b'\n').count() + 1
}

/// Offset just past the closing quote of the ordinary string starting at `start`.
fn end_of_string(src: &str, start: usize) -> usize {
    let b = src.as_bytes();
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    panic!(
        "a string opened at line {} never closes; this guard cannot tell code from text in that file",
        line_of(src, start)
    )
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
        // `'\n'`, `'\''`, `'\x41'`, `'\u{1F600}'` — the closing quote is the next unescaped one,
        // and a lifetime never contains a backslash, so the shape alone decides it.
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

/// Every place `hub.rs` names the transport, as `(line, spelling)`.
fn hub_findings(code: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        for spelling in NOT_IN_THE_HUB {
            if line.contains(spelling) {
                found.push((n + 1, spelling));
            }
        }
    }
    found
}

/// Every place a file of the hub's reaches for code this scan does not walk, as `(line, directive)`.
fn directive_findings(code: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        for directive in NO_CODE_FROM_ELSEWHERE {
            // A `cfg_attr` is only a way out of the hub when what it carries is a `path`; the rest
            // of the time — `#[cfg_attr(test, derive(Debug))]` — it is an ordinary attribute, and a
            // rule that tripped on every one of those would be switched off within a week. That is
            // how a guard dies, and this one is meant to outlive the people who wrote it.
            if directive == "cfg_attr" && !line.contains("path") {
                continue;
            }
            if line.contains(directive) {
                found.push((n + 1, directive));
            }
        }
    }
    found
}

/// Every place `transport.rs` names something only the hub decides, as `(line, word)`.
fn transport_findings(code: &str) -> Vec<(usize, String)> {
    let forbidden: BTreeSet<&str> = NOT_IN_THE_TRANSPORT.into_iter().collect();
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        for word in words_of(line) {
            if forbidden.contains(word.as_str()) {
                found.push((n + 1, word));
            }
        }
    }
    found
}

/// The lowercased words in a line, splitting identifiers on `_`, on digits and on camel humps.
///
/// `AskId` → `ask`, `id`. `asks_json` → `asks`, `json`. `task` stays `task`, which is the whole
/// reason this is not a substring match.
fn words_of(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in line.split(|c: char| !c.is_ascii_alphabetic()) {
        if run.is_empty() {
            continue;
        }
        let cs: Vec<char> = run.chars().collect();
        let mut start = 0;
        for i in 1..cs.len() {
            let camel_hump = cs[i - 1].is_lowercase() && cs[i].is_uppercase();
            let end_of_shout = cs[i - 1].is_uppercase()
                && cs[i].is_uppercase()
                && cs.get(i + 1).is_some_and(|c| c.is_lowercase());
            if camel_hump || end_of_shout {
                out.push(cs[start..i].iter().collect::<String>().to_lowercase());
                start = i;
            }
        }
        out.push(cs[start..].iter().collect::<String>().to_lowercase());
    }
    out
}

/// Everything wrong with the seam, in one list. Empty means the seam holds.
///
/// One list rather than an early panic, because the first run of this scan reports both halves at
/// once and a builder fixing them wants to see all of it.
fn scan_the_seam(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    match read_source(root, HUB) {
        Err(why) => problems.push(why),
        Ok(src) => {
            let code = code_only(&src);
            for anchor in HUB_ANCHORS {
                if !code.contains(anchor) {
                    problems.push(format!(
                        "{HUB} no longer contains `{anchor}`. Either it stopped being the layer this \
                         guard is about, or the scan is reading the wrong thing — either way it \
                         cannot prove the seam holds."
                    ));
                }
            }
            for (line, spelling) in hub_findings(&code) {
                problems.push(format!(
                    "{HUB}:{line} names `{spelling}`. The transport owns that word now: take it \
                     from {TRANSPORT} (a `ConnectionIdentity`, an `Accepted`, a `ByteStream`) \
                     instead of reaching for the socket here."
                ));
            }
            for (line, directive) in directive_findings(&code) {
                problems.push(format!(
                    "{HUB}:{line} uses `{directive}`, which brings code into the hub from a file \
                     this scan does not walk. The hub is {HUB} and {HUB_DIR}; put it there."
                ));
            }
        }
    }

    let (module_files, mut walk_problems) = hub_module_files(root);
    problems.append(&mut walk_problems);
    for relative in module_files {
        match read_source(root, &relative) {
            Err(why) => problems.push(why),
            Ok(src) => {
                let code = code_only(&src);
                for (line, spelling) in hub_findings(&code) {
                    problems.push(format!(
                        "{relative}:{line} names `{spelling}`. It is part of the hub, so the same \
                         rule applies: the transport owns that word, and moving a socket into a \
                         module beside the hub does not move it out of the hub."
                    ));
                }
                for (line, directive) in directive_findings(&code) {
                    problems.push(format!(
                        "{relative}:{line} uses `{directive}`, which brings code into the hub from \
                         a file this scan does not walk. The hub is {HUB} and {HUB_DIR}; put it \
                         there."
                    ));
                }
            }
        }
    }

    match read_source(root, TRANSPORT) {
        Err(why) => problems.push(why),
        Ok(src) => {
            let code = code_only(&src);
            for anchor in TRANSPORT_ANCHORS {
                if !code.contains(anchor) {
                    problems.push(format!(
                        "{TRANSPORT} no longer contains `{anchor}`. Either it stopped being the \
                         transport, or the scan is reading the wrong thing — either way it cannot \
                         prove the seam holds."
                    ));
                }
            }
            for (line, word) in transport_findings(&code) {
                problems.push(format!(
                    "{TRANSPORT}:{line} names `{word}`. What a claim, an ask, a topic or a registry \
                     is belongs to {HUB}; the transport carries bytes and says who sent them."
                ));
            }
        }
    }

    // And the transport's own module directory, on the same argument [`HUB_DIR`] rests on: the
    // gateway of `docs/CAPABILITIES.md` OPEN 4 is the change that splits one file into several, and
    // a scan that only ever read `transport.rs` would go blind on the commit that did it.
    let (transport_files, mut transport_walk_problems) =
        rust_files_under(root, TRANSPORT_DIR, true);
    problems.append(&mut transport_walk_problems);
    for relative in transport_files {
        match read_source(root, &relative) {
            Err(why) => problems.push(why),
            Ok(src) => {
                for (line, word) in transport_findings(&code_only(&src)) {
                    problems.push(format!(
                        "{relative}:{line} names `{word}`. It is part of the transport, so the same \
                         rule applies: what a claim, an ask, a topic or a registry is belongs to \
                         {HUB}."
                    ));
                }
            }
        }
    }

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
fn the_hub_knows_nothing_of_a_unix_socket_and_the_transport_knows_nothing_of_a_claim() {
    let problems = scan_the_seam(&workspace_root());
    assert!(
        problems.is_empty(),
        "the transport seam has leaked, in {} place(s):\n{}",
        problems.len(),
        report(&problems)
    );
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The guard's own guards. A scanner nobody attacked is a scanner that reports green.

#[test]
fn a_seam_whose_files_are_missing_fails_the_scan_instead_of_finding_nothing() {
    let nowhere = tempfile::tempdir().expect("a temp dir");
    let problems = scan_the_seam(nowhere.path());
    for expected in [HUB, TRANSPORT] {
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with(expected) && p.contains("could not be read")),
            "a scan that could not open {expected} reported this instead:\n{}",
            report(&problems)
        );
    }
}

#[test]
fn a_file_that_no_longer_holds_what_it_is_named_for_fails_rather_than_scanning_a_stranger() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let src = dir.path().join("crates/kickoff-channel/src");
    std::fs::create_dir_all(&src).expect("a place to put the two files");
    // Both files clean of every forbidden spelling, and both empty of the thing they are for. A
    // guard that scored this green is a guard that would score a renamed hub green.
    std::fs::write(src.join("hub.rs"), "fn nothing() {}\n").expect("write the stand-in hub");
    std::fs::write(src.join("transport.rs"), "fn nothing() {}\n")
        .expect("write the stand-in transport");

    let problems = scan_the_seam(dir.path());
    for anchor in ["async fn serve_connection(", "ConnectionIdentity"] {
        assert!(
            problems.iter().any(|p| p.contains(anchor)),
            "a scan of files holding none of what they are named for missed `{anchor}`:\n{}",
            report(&problems)
        );
    }
}

/// A tree shaped like this repo's with both halves clean, to plant one defect at a time in.
///
/// The hub's own test module is written WITH a socket in it, because that is the real arrangement:
/// it is the one file under the hub that is allowed one, and a rig that left it out would never
/// notice the exemption silently widening.
fn a_tree_whose_seam_holds() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    let src = dir.path().join("crates/kickoff-channel/src");
    std::fs::create_dir_all(src.join("hub")).expect("a place to put the stand-in files");
    std::fs::write(
        src.join("hub.rs"),
        "async fn serve_connection() {}\nstruct Claim {}\n",
    )
    .expect("write the stand-in hub");
    std::fs::write(
        src.join("transport.rs"),
        "struct ConnectionIdentity;\nstruct LocalSocket;\n",
    )
    .expect("write the stand-in transport");
    std::fs::write(
        src.join("hub/tests.rs"),
        "fn over_a_real_socket(s: UnixStream) { let _ = s.into_split(); }\n",
    )
    .expect("write the stand-in hub tests");
    dir
}

#[test]
fn a_socket_moved_into_a_module_beside_the_hub_is_found_there_too() {
    let dir = a_tree_whose_seam_holds();
    let clean = scan_the_seam(dir.path());
    assert!(
        clean.is_empty(),
        "a tree whose seam holds was reported as leaking — and the only socket in it is the hub's \
         own test module, which is the one exemption:\n{}",
        report(&clean)
    );

    // Splitting the hub into modules is the ordinary change that would take this guard's sight
    // away if it only ever read `hub.rs`.
    std::fs::write(
        dir.path().join("crates/kickoff-channel/src/hub/door.rs"),
        "fn open() -> UnixListener { todo!() }\n",
    )
    .expect("plant a socket in a module beside the hub");

    let problems = scan_the_seam(dir.path());
    assert!(
        problems.iter().any(
            |p| p.starts_with("crates/kickoff-channel/src/hub/door.rs:1")
                && p.contains("UnixListener")
        ),
        "a socket planted in a module beside the hub was not reported:\n{}",
        report(&problems)
    );
}

/// The two ways to put code in the hub without putting a file in the hub's directory.
///
/// This is not hypothetical here: the sibling guard in `no_live_write_call_site.rs` was walked past
/// six times, and two of them were exactly these. `include!` splices a file into `hub.rs`'s own
/// module and `#[path]` mounts one from anywhere, so either puts a `UnixListener` in the shipped
/// binary, inside the hub, with this scan reporting green.
#[test]
fn code_pulled_into_the_hub_from_a_file_this_scan_does_not_walk_is_refused() {
    for directive in [
        "include!(\"../hub_door.rs\");",
        "#[path = \"../hub_door.rs\"] mod door;",
        "#[cfg_attr(unix, path = \"../hub_door.rs\")] mod door;",
    ] {
        let dir = a_tree_whose_seam_holds();
        let hub = dir.path().join(HUB);
        let mut src = std::fs::read_to_string(&hub).expect("read the stand-in hub");
        src.push_str(directive);
        src.push('\n');
        std::fs::write(&hub, src).expect("plant the directive");

        let problems = scan_the_seam(dir.path());
        assert!(
            problems.iter().any(|p| p.starts_with(HUB)),
            "`{directive}` brings code into the hub from a file this scan never opens, and the scan \
             said nothing:\n{}",
            report(&problems)
        );
    }
}

/// A directory named like something to skip — the shape that walked past the sibling guard four
/// times. `file_type()` is `lstat`-based, so a symlink to a directory is neither a directory nor a
/// `.rs` file, and a walk that only asks those two questions goes past it in silence.
#[test]
fn a_symlinked_directory_under_the_hub_is_refused_rather_than_walked_past_in_silence() {
    let dir = a_tree_whose_seam_holds();
    let elsewhere = dir.path().join("crates/kickoff-channel/src/hub_parts");
    std::fs::create_dir_all(&elsewhere).expect("a directory outside the hub");
    std::fs::write(
        elsewhere.join("door.rs"),
        "fn open() -> UnixListener { todo!() }\n",
    )
    .expect("a socket in it");
    std::os::unix::fs::symlink(
        "../hub_parts",
        dir.path().join("crates/kickoff-channel/src/hub/parts"),
    )
    .expect("link it into the hub");

    let problems = scan_the_seam(dir.path());
    assert!(
        !problems.is_empty(),
        "a directory linked into the hub was skipped without a word; the guard reported a clean \
         scan of a tree it had not finished looking at"
    );
}

/// The transport is one file today and the gateway of `docs/CAPABILITIES.md` OPEN 4 is what splits
/// it. The half of this guard that keeps the transport ignorant of a claim must not go blind on the
/// commit that does it — which is the same argument [`HUB_DIR`] exists for, on the other side.
#[test]
fn a_transport_split_into_modules_is_scanned_like_the_one_file_it_grew_out_of() {
    let dir = a_tree_whose_seam_holds();
    std::fs::create_dir_all(dir.path().join("crates/kickoff-channel/src/transport"))
        .expect("a transport module directory");
    std::fs::write(
        dir.path()
            .join("crates/kickoff-channel/src/transport/gateway.rs"),
        "fn who_holds_the_claim() {}\n",
    )
    .expect("plant a claim in a module beside the transport");

    let problems = scan_the_seam(dir.path());
    assert!(
        problems.iter().any(
            |p| p.starts_with("crates/kickoff-channel/src/transport/gateway.rs:1")
                && p.contains("claim")
        ),
        "a claim planted in a module beside the transport was not reported:\n{}",
        report(&problems)
    );
}

#[test]
fn a_hub_test_module_that_has_gone_missing_fails_rather_than_widening_the_exemption() {
    let dir = a_tree_whose_seam_holds();
    std::fs::remove_file(dir.path().join(HUB_DIR_EXEMPT)).expect("take the exempt file away");
    let problems = scan_the_seam(dir.path());
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with(HUB_DIR_EXEMPT) && p.contains("is not there")),
        "the one exempt file vanished and the scan carried on regardless:\n{}",
        report(&problems)
    );
}

#[test]
fn a_forbidden_spelling_planted_in_either_half_is_found_by_this_guard() {
    for spelling in NOT_IN_THE_HUB {
        let planted = format!("fn f() {{ let x = {spelling}; }}\n");
        let found = hub_findings(&code_only(&planted));
        assert!(
            found.iter().any(|(_, s)| *s == spelling),
            "`{spelling}` is in the forbidden list but planting it in the hub found nothing — the \
             list would be scanning for a word that cannot match"
        );
    }
    for word in NOT_IN_THE_TRANSPORT {
        let planted = format!("fn f() {{ let {word}_x = 1; }}\n");
        let found = transport_findings(&code_only(&planted));
        assert!(
            found.iter().any(|(_, w)| w == word),
            "`{word}` is in the forbidden list but planting it in the transport found nothing"
        );
    }
}

/// The other half of the directive rule: an attribute that carries no path is not a way out of the
/// hub, and a guard that said it was would be turned off rather than obeyed.
#[test]
fn an_ordinary_attribute_that_carries_no_path_is_not_code_coming_in_from_elsewhere() {
    let innocent = "#[cfg_attr(test, derive(Debug))]\nstruct Claim {}\n";
    assert!(
        directive_findings(&code_only(innocent)).is_empty(),
        "an ordinary conditional attribute was read as a module pulled in from outside the hub"
    );
}

#[test]
fn a_word_that_merely_contains_a_forbidden_word_is_not_a_forbidden_word() {
    // The transport will spawn tasks and read frames someone asked for; if this scan tripped on
    // those it would be turned off within a week, which is the failure mode a noisy guard has.
    let innocent = "fn f() { tokio::task::spawn(async { let asked = basket(); }); }\n";
    let found = transport_findings(&code_only(innocent));
    assert!(
        found.is_empty(),
        "the transport scan tripped on an ordinary word: {found:?}"
    );
}

#[test]
fn a_forbidden_word_in_a_comment_is_prose_but_the_same_word_in_a_string_is_code() {
    let commented = concat!(
        "// the hub holds the claim; this file only opens the door\n",
        "/* an ask, a topic, a registry: none of it lives here */\n",
        "/// the claim's owner asked for this\n",
        "fn door() {}\n"
    );
    assert!(
        transport_findings(&code_only(commented)).is_empty(),
        "a comment explaining WHY the transport knows nothing of a claim was read as the transport \
         knowing about one"
    );

    let stringed = "fn f() -> &'static str { \"the topic he typed in\" }\n";
    assert!(
        !transport_findings(&code_only(stringed)).is_empty(),
        "a forbidden word inside a string literal was blanked along with the comments"
    );

    let hub_string = "fn p() -> String { format!(\"/run/user/{uid}/kickoff/hub.sock\") }\n";
    assert!(
        !hub_findings(&code_only(hub_string)).is_empty(),
        "the socket path is a string literal, and a scan that skips strings cannot see it"
    );
}

#[test]
fn a_char_literal_holding_a_quote_does_not_blind_the_rest_of_the_file() {
    // `'"'` read as the start of a string swallows everything after it, and the scan then finds
    // nothing in a file full of findings — silently, which is the whole class of failure here.
    let src = "fn q(c: char) -> bool { c == '\"' }\nfn r() { let s = UnixStream; }\n";
    let found = hub_findings(&code_only(src));
    assert_eq!(
        found,
        vec![(2, "UnixStream")],
        "a char literal holding a quote hid the line after it"
    );
}

#[test]
fn a_raw_string_full_of_quotes_does_not_blind_the_rest_of_the_file() {
    let src = "fn q() -> &'static str { r#\"a \" and a \\ and a // \"# }\nfn r() { let s = UnixListener; }\n";
    let found = hub_findings(&code_only(src));
    assert_eq!(
        found,
        vec![(2, "UnixListener")],
        "a raw string hid the line after it"
    );
}

#[test]
fn the_hub_as_it_stood_before_the_transport_moved_out_is_still_caught_by_this_scan() {
    let root = workspace_root();
    let src = hub_at(&root, BEFORE_THE_SEAM);
    let code = code_only(&src);
    let found: BTreeSet<&str> = hub_findings(&code).into_iter().map(|(_, s)| s).collect();

    // Every spelling that file really contained, in code rather than in prose. This is what stops
    // the forbidden list from rotting into words that match nothing: if a rename made one of these
    // unfindable, the list is wrong and this test says which entry.
    for spelling in [
        "UnixStream",
        "UnixListener",
        "socket_peercred",
        "peer_cred",
        "PeerCred",
        "into_split",
        "socket_path",
        "hub.sock",
        "/run/user",
    ] {
        assert!(
            found.contains(spelling),
            "the hub at {BEFORE_THE_SEAM} contained `{spelling}` and this scan did not report it; \
             the scan, not the history, is what changed"
        );
    }

    // And the counterpart: `SO_PEERCRED` appears in that file exactly once, in the module doc
    // describing gate 1. Comments are prose, so it must NOT be a finding — this is the assertion
    // that the comment blanking really happens rather than being assumed.
    assert!(
        src.contains("SO_PEERCRED"),
        "the hub at {BEFORE_THE_SEAM} was expected to name SO_PEERCRED in its module doc"
    );
    assert!(
        !found.contains("SO_PEERCRED"),
        "SO_PEERCRED was only ever in a comment in that file, so reporting it means comments are \
         being scanned as code"
    );
}

/// `hub.rs` as it was at `commit`, read out of this repository.
///
/// **Panics rather than skipping** when git cannot answer. A guard whose only witness is
/// unreachable has no witness, and "I could not look" must never come out as "I looked".
fn hub_at(root: &Path, commit: &str) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!("{commit}:{HUB_BEFORE_THE_SEAM}"));
    // `GIT_DIR` and friends are exported into every hook git runs and WIN over `-C`, so inside the
    // pre-commit hook this witness would otherwise be about whatever repository they name.
    for leaked in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_PREFIX",
    ] {
        cmd.env_remove(leaked);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("git could not be run to read the hub at {commit}: {e}"));
    assert!(
        out.status.success(),
        "git could not read {HUB_BEFORE_THE_SEAM} at {commit}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8(out.stdout).expect("the hub is UTF-8")
}

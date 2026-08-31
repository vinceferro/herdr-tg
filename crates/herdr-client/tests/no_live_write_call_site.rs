//! **D3's catastrophic-failure guard, asserted rather than asserted-in-prose.**
//!
//! `pane.send_text` / `pane.send_keys` / `pane.send_input` type real keystrokes into the operator's
//! REAL terminals, where REAL agents are working. Slice 1 ships them built, typed and mock-tested
//! with **no live call site and no binary subcommand that reaches them** (spec delta #24). That is
//! a structural property, and a structural property that only a comment defends is one refactor
//! away from being false.
//!
//! # DENY BY DEFAULT — the whole workspace, not an allowlist of two directories
//!
//! The first version of this file scanned exactly two hardcoded directory literals,
//! `crates/herdr-client/src` and `crates/herdr-tg/src`. A reviewer walked straight past it: a THIRD
//! workspace member calling `send_text` left the suite green, and so did the same call planted in
//! `tests/`, `examples/` or `build.rs` of the two crates it did scan. Slice 2 adds exactly that
//! shape — a new bot crate — so the guard would have gone blind on the commit that needed it most.
//!
//! So the walk now starts at the WORKSPACE ROOT and scans every `.rs` file it finds — plus every
//! file a `include!` or a `#[path]` pulls into a crate, whatever it is called — skipping only REAL
//! cargo build directories (a `target/` beside a Cargo.toml) and `.git/`. A crate added tomorrow
//! is covered the day it is added rather than the day someone remembers to add it here. Five
//! properties make that real rather than nominal:
//!
//! 1. **An unreadable directory or file FAILS.** The old walk had `Err(_) => continue`, so a
//!    permission error read as "nothing to see". A guard that cannot see must not report green.
//! 2. **Every declared workspace member must contribute at least one scanned file.** A renamed or
//!    moved crate makes the scan vacuous otherwise — green because it looked nowhere.
//! 3. **The exemptions are a short explicit list of FILES, asserted to exist.** Not a directory
//!    prefix, not a pattern. If `wire.rs` is renamed its exemption goes stale, the new file gets
//!    scanned, and the suite goes red — the safe direction.
//! 4. **The walk's coverage is cross-checked against a source of truth it had no hand in.** Rules
//!    that decide what NOT to look at have now been wrong four times here — an allowlist of two
//!    directories, a member list nobody verified, a bare-name `target` skip, a `.rs` extension
//!    filter. Each round fixed the instance and left the class open, because nothing outside the
//!    walk could contradict the walk.
//!    [`every_file_that_ships_is_scanned_or_carries_no_write`] compares what was scanned against
//!    the git index, which is by definition what ships, so the NEXT silent skip is loud on the
//!    commit that introduces it rather than in the review after it.
//! 5. **The cross-check does not share the walk's assumptions.** The first version of it filtered
//!    the index down to `.rs` — the very rule it was auditing — so a tracked `relay.inc` holding a
//!    live `send_text`, one `include!` line from the shipped binary, was invisible to the walk and
//!    to its own second opinion at the same time. An oracle that shares its subject's blind spot
//!    is not an oracle. It now judges a file by whether the guard READ it, not by what it is
//!    called, and it fails rather than skipping when it cannot reach git at all.
//! 6. **A lookup that comes up empty is a FAILURE, never a shrug.** Five distinct ways past this
//!    guard have now been found, and most of them ended the same way: something the guard was
//!    asked to resolve did not resolve, and the code quietly carried on. An `include!` it could
//!    not parse. A `#[path]` resolved against the wrong directory, so the file rustc compiles was
//!    never opened. A closing brace looked for at an indentation `#[rustfmt::skip]` had changed.
//!    A git index it could not read — and, found while fixing those, a `git` question answered by
//!    whatever repository `GIT_DIR` named instead of the one being guarded. Every one of those is
//!    now loud, because a guard that says nothing when it does not understand the code is worse
//!    than no guard: it is believed.
//!
//! # Known and accepted: this guard does not read a member's Cargo.toml
//!
//! Its model of "what is code" is (every `.rs` under the root) ∪ (whatever an `include!` or a
//! `#[path]` names). It never reads a workspace member's own manifest, so a Cargo TARGET PATH —
//! `[[bin]]`, `[lib]`, `[[example]]`, `[[test]]` with a `path = …` — is a third way for a file to
//! become code that nothing here models. A file that is BOTH outside every member directory AND
//! not spelled `.rs` is invisible to every check at once: the walk filters on the extension, and
//! the git oracle's extension-blind clause only reaches inside member directories.
//!
//! That gap is KNOWN and ACCEPTED — a decision, not an oversight. This guard stays a source
//! scanner; it does not grow a second manifest parser to chase it. What it costs is bounded on
//! purpose: BOTH textual rules walk from the workspace ROOT, so an out-of-tree target that is
//! spelled `.rs` — the ordinary shared-tooling layout — is read like any other file, whatever
//! directory it sits in. The residue is the out-of-tree target that is also not `.rs`. It is
//! written down here so the next reader neither believes the guard covers it nor re-derives it
//! from scratch.
//!
//! # The three rules
//!
//! 1. **No file outside `herdr-client` may so much as NAME the three methods.** Not a call, not an
//!    import, not a clap subcommand, not a `//` TODO that a later session turns into one.
//!    Everything outside the defining crate is what a timer, a cron job or (from slice 2) a
//!    Telegram message can reach; the client crate, which defines them, is not. Scoped by the
//!    DEFINING crate rather than by a list of member directories, because a member's source file
//!    need not live inside its own directory.
//! 2. **A *call* may appear nowhere in the workspace outside `#[cfg(test)]`** — including via UFCS
//!    (`HerdrClient::send_text(c, p, …)`), which the old `.send_text(`-only spelling missed.
//! 3. **`Request` must stay SEALED.** The seal is what makes rules 1 and 2 more than a grep: an
//!    unsealed public `Request` lets any crate hand `HerdrClient::call` a `const METHOD` that never
//!    spells `send_text` at all (`concat!("pane.send", "_text")`), so no textual rule can see it.
//!    Rule 3 is a tripwire on the declaration; the compiler is the actual enforcement.
//!
//! It reads only files inside this repository. It opens no socket and touches no herd.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// The three methods, spelled as they appear anywhere at all.
const WRITE_NAMES: [&str; 3] = ["send_text", "send_keys", "send_input"];

/// Files excused from the CALL rule, workspace-relative and exact.
///
/// Deliberately short, deliberately not a prefix. Each is asserted to exist, so a rename shows up
/// as a red test rather than as an exemption that silently stops applying to anything.
const CALL_RULE_EXEMPT: [&str; 2] = [
    // The ONLY mock-backed exercise of the three write RPCs. Without it the write path ships with
    // its wire shape unasserted, which is worse than the exemption.
    "crates/herdr-client/tests/wire.rs",
    // This file. It has to spell the names to look for them.
    "crates/herdr-client/tests/no_live_write_call_site.rs",
];

/// The crate that DEFINES the three methods, and so is the one place allowed to name them.
const DEFINING_MEMBER: &str = "crates/herdr-client";

/// The ONE file outside the defining crate that may reach a write, and why.
///
/// Slice 1 shipped the writes with no path from any reachable crate — the invariant was simply
/// "nowhere". Slice 3 gives the operator a reply path, so "nowhere" became false, and an invariant
/// that is false is an invariant that gets deleted. It is replaced by a narrower one that is still
/// worth having: **every write goes through the single module that verifies delivery and audits
/// it.** A Telegram message can now reach a keystroke, but only along a path that reads the pane
/// back, refuses to claim more than it observed, and writes an audit record.
///
/// Widening this list is a decision about the operator's terminals. It should feel like one.
const AUDITED_WRITE_PATH: &str = "crates/herdr-tg/src/deliver.rs";

/// The workspace root: `crates/herdr-client/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-client is two levels below the workspace root")
        .to_path_buf()
}

/// Is `parent/name` a directory the walk may skip?
///
/// A name alone is never enough, and that is the entire point of this function. The previous rule
/// was a two-name list matched against the bare directory name at ANY depth, so
/// `crates/herdr-tg/src/target/` — an ordinary Rust module, in a crate that already has a `Target`
/// enum — was pruned out of the tree, and a live `send_text` sat inside it with every test green.
///
/// `target` is skipped ONLY when its parent holds a `Cargo.toml`, i.e. when it is the build output
/// of the crate or workspace directly above it (`<root>/target`, `crates/<member>/target`). A
/// `src/target/` has no sibling manifest and is walked like any other module.
///
/// `.git` is skipped on the name alone, and that is safe for a reason that is CHECKED rather than
/// assumed: git refuses to track any path with a `.git` component, so this skip cannot hide a file
/// that ships, and the git cross-check proves it on every run.
///
/// One hole is left open here deliberately: a `Cargo.toml` dropped beside a `src/target/` would
/// buy the skip back. The rule stays this simple and this readable, and
/// [`every_file_that_ships_is_scanned_or_carries_no_write`] is the backstop — that module would be
/// tracked and unwalked, and it fails. Read the two together; neither is enough on its own, and
/// the oracle's own blind spot (a file that is neither tracked nor walked, which cannot ship but
/// can be built locally) is what this rule closes. Deleting either half reopens the other's.
fn is_skippable_dir(parent: &Path, name: &OsStr) -> bool {
    if name == OsStr::new(".git") {
        return true;
    }
    name == OsStr::new("target") && parent.join("Cargo.toml").is_file()
}

/// Every `.rs` file under `dir`, sorted so a failure names the same file every run.
///
/// **Panics on an unreadable directory.** That is the point: the previous version swallowed the
/// error and carried on, which turns "I could not look" into "I looked and it was clean".
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = fs::read_dir(&d).unwrap_or_else(|e| {
            panic!(
                "the D3 write guard could not read {} ({e}). A guard that cannot see the tree must \
                 not report green — fix the path or the permissions.",
                d.display()
            )
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| {
                panic!(
                    "the D3 write guard could not stat an entry under {} ({e})",
                    d.display()
                )
            });
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if is_skippable_dir(&d, &name) {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Byte offset of the first non-whitespace byte at or after `i`.
fn skip_ws(line: &str, mut i: usize) -> usize {
    while line.as_bytes().get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// `p` with every `.` and `..` component resolved textually.
///
/// Not `fs::canonicalize`: that resolves symlinks too, so the result no longer starts with the
/// workspace root the rest of this file compares against.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The files a CODE-inclusion directive in `src` names, resolved the way RUSTC resolves them.
///
/// `include!("relay.inc")` and `#[path = "relay.txt"] mod relay;` both make a file that is not
/// spelled `.rs` into code inside a crate. That is the hole a reviewer walked through: a tracked
/// `relay.inc` holding a live `send_text`, one `include!` line away from the shipped binary, with
/// the walk collecting only `.rs` files and BOTH git oracles filtering the index down to `.rs`
/// as well — the oracle sharing the exact assumption it existed to contradict.
///
/// # Where a `#[path]` points, and why getting that wrong was silent
///
/// The next reviewer walked through the RESOLUTION rather than the detection. A `#[path]` was
/// resolved against the naming file's own directory, which is only right at the TOP LEVEL of a
/// file. Written inside an inline `mod { }` block, rustc resolves it against the directory plus
/// the inline module path — and in a file that is not a mod-rs file (`mod.rs`, `lib.rs`,
/// `main.rs`), against the file's STEM as well: `<dir>/<stem>/<inline mods>/`. The guard probed a
/// directory that does not exist, and because `path` was read loosely that miss was a silent
/// `continue`. A tracked file, compiled into the shipped binary, holding a live `send_text`, that
/// the guard never opened — with all five gates green.
///
/// So a redirect is now resolved against every base rustc could be using, and scanned wherever it
/// lands. A file that is not `mod.rs`/`lib.rs`/`main.rs` may still be a crate root (a bin, a test,
/// an example), where the stem is NOT inserted, so both spellings are probed. Probing more bases
/// than rustc uses costs nothing: scanning an extra file has never been the failure here.
///
/// # The two directives are read with different strictness, because they are different words
///
/// * **`include!` is unambiguous.** It names code and nothing else, so it is parsed strictly: all
///   three bracket shapes, then a plain string literal, then a file that exists inside the
///   workspace. Anything else — a `concat!(env!("OUT_DIR"), …)`, an argument on the following
///   line, a path climbing out of the tree — is a hard FAIL. The guard saw a directive and could
///   not read it, and "I could not look" must never come out as "I looked and it was clean". This
///   is the same treatment a glob workspace member gets.
/// * **`path` is an ordinary English word**, so it is FOUND loosely and then judged by what it is
///   attached to. `let path = "…"` is not a module redirect and never fails the guard. A
///   `#[path = "…"]` — or `#[cfg_attr(…, path = "…")]`, the same thing under another spelling —
///   sitting on a `mod` IS one: it names a file the compiler WILL read, so if no candidate base
///   yields a file inside the workspace, the guard has not looked and it FAILS. The old doc argued
///   that a `#[path]` naming a missing file cannot compile. That was only ever true of the one
///   path the guard happened to probe.
///
/// A `#[path]` on an INLINE `mod name { … }` names a DIRECTORY, not a file: it replaces the
/// component the module's own name would have contributed for everything nested inside it. It is
/// tracked as that component, and any redirect nested under it is checked in the usual way.
///
/// `include_str!` / `include_bytes!` are deliberately NOT followed: they produce DATA, not code,
/// so the method name inside one cannot be a call site — and the crate's JSON schema fixture
/// legitimately carries `"const": "pane.send_text"`. What stops that data from becoming a method
/// name on the wire is [`the_request_trait_is_sealed_so_no_foreign_crate_can_choose_a_method`],
/// not this function.
fn code_includes(root: &Path, file: &Path, src: &str) -> Vec<PathBuf> {
    let dir = file
        .parent()
        .expect("a scanned file has a parent directory")
        .to_path_buf();
    let stem = file
        .file_stem()
        .expect("a scanned file has a name")
        .to_owned();
    // The three names rustc treats as "this file speaks for its directory". Everything else adds
    // its own stem as a directory for the modules nested inside it — unless it is a crate root
    // (a bin, a test, an example), which the guard cannot tell apart from here. So it probes both.
    let is_mod_rs = matches!(
        file.file_name().and_then(OsStr::to_str),
        Some("mod.rs" | "lib.rs" | "main.rs")
    );

    // Every directory rustc could be resolving `named` against, given the inline `mod` blocks we
    // are currently inside, and which of them actually hold the file.
    let resolve = |named: &str, blocks: &[(usize, String)]| -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut nested = dir.clone();
        for (_, component) in blocks {
            nested.push(component);
        }
        let mut bases = vec![nested];
        if !blocks.is_empty() && !is_mod_rs {
            let mut with_stem = dir.join(&stem);
            for (_, component) in blocks {
                with_stem.push(component);
            }
            bases.push(with_stem);
        }
        let mut hits = Vec::new();
        let mut tried = Vec::new();
        for base in bases {
            let target = normalise(&base.join(named));
            if target.starts_with(root) && target.is_file() {
                hits.push(target.clone());
            }
            tried.push(target);
        }
        (hits, tried)
    };

    // The string literal at `line[at..]`, read out of the ORIGINAL line because `code_only`
    // blanked its contents. Blanking preserves byte offsets, so the two agree on where it is.
    let literal_at = |line: &str, at: usize| -> Option<(String, usize)> {
        if line.as_bytes().get(at) != Some(&b'"') {
            return None;
        }
        let body = at + 1;
        let len = line[body..].find('"')?;
        Some((line[body..body + len].to_owned(), body + len + 1))
    };

    let mut out = Vec::new();
    // The inline `mod` components we are inside, each with the brace depth it opened at.
    let mut blocks: Vec<(usize, String)> = Vec::new();
    let mut depth = 0usize;
    // A `path = "…"` read out of an attribute, waiting to see what item it lands on.
    let mut pending: Option<String> = None;
    // A `mod NAME` waiting for the `;` or `{` that says whether it is a file or an inline block.
    let mut opening: Option<(String, Option<String>)> = None;
    // Bracket nesting inside a `#[ … ]`; 0 means we are not in an attribute.
    let mut in_attr = 0usize;

    // The whole-file view, so a directive written inside a multi-line string is prose and the
    // brace counting that tracks the inline modules is not fooled by a `{` in one.
    for (line, code) in src.lines().zip(code_lines(src)) {
        let b = code.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if in_attr > 0 && (c == b'[' || c == b']') {
                if c == b'[' {
                    in_attr += 1;
                } else {
                    in_attr -= 1;
                }
                i += 1;
                continue;
            }
            // `#[…]` and `#![…]` both open an attribute.
            if c == b'#' {
                let mut j = i + 1;
                if b.get(j) == Some(&b'!') {
                    j += 1;
                }
                if b.get(j) == Some(&b'[') {
                    in_attr = 1;
                    i = j + 1;
                    continue;
                }
                i += 1;
                continue;
            }
            if in_attr > 0 && (c == b'{' || c == b'}') {
                // An attribute's own braces are not module braces. Counting them would move the
                // inline module path a `#[path]` below is resolved against.
                i += 1;
                continue;
            }
            if c == b'{' {
                depth += 1;
                if let Some((name, redirect)) = opening.take() {
                    // A `#[path]` on an inline module renames the DIRECTORY its children resolve
                    // through; without one, the module's own name is that directory.
                    blocks.push((depth, redirect.unwrap_or(name)));
                }
                i += 1;
                continue;
            }
            if c == b'}' {
                while blocks.last().is_some_and(|(d, _)| *d == depth) {
                    blocks.pop();
                }
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            if c == b';' {
                if let Some((_, Some(named))) = opening.take() {
                    // A file module the compiler WILL read. Not finding it means the guard has not
                    // looked at code that ships, which is the one thing it must never do quietly.
                    let (hits, tried) = resolve(&named, &blocks);
                    assert!(
                        !hits.is_empty(),
                        "`#[path = \"{named}\"]` in {} redirects a module at a file this guard \
                         cannot find. rustc would read it from one of:\n  {}\nNone of those is a \
                         file inside the workspace, so the guard has NOT read code the compiler \
                         compiles into the operator's binary. Fix the path, or teach this \
                         function the shape deliberately — do not let it through unscanned.",
                        file.display(),
                        tried
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n  ")
                    );
                    out.extend(hits);
                }
                i += 1;
                continue;
            }
            if !(c.is_ascii_alphanumeric() || c == b'_') {
                i += 1;
                continue;
            }

            // An identifier, consumed WHOLE — which is what tells `include!` from `include_str!`
            // and `my_include!`, and `path` from `pathological`.
            let mut end = i;
            while b
                .get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                end += 1;
            }
            let word = &code[i..end];

            if word == "include" && b.get(end) == Some(&b'!') {
                out.push(strict_include_target(root, file, line, end + 1));
                i = end + 1;
                continue;
            }
            if word == "path" {
                // Read the blanked view for what FOLLOWS the word, so a comment sitting between it
                // and the `=` is not mistaken for the thing it is attached to. Offsets are shared.
                let mut j = skip_ws(&code, end);
                assert!(
                    j < code.len() || in_attr == 0,
                    "`path` inside an attribute in {} is the last thing on its line:\n  {}\nThe \
                     `=` and the file it names are on the NEXT line, and this guard will not guess \
                     which file the module is redirected to. A `#[path]` names code in the \
                     operator's binary, so teach this function the shape deliberately rather than \
                     letting the file through unscanned.",
                    file.display(),
                    line.trim()
                );
                if code.as_bytes().get(j) == Some(&b'=') {
                    // Back to the ORIGINAL line for the literal itself: the blanked view has its
                    // body — and its opening quote — replaced by spaces, so skipping whitespace
                    // there would walk straight past the thing we are looking for.
                    j = skip_ws(line, j + 1);
                    match literal_at(line, j) {
                        Some((named, after)) => {
                            // Loose: whatever it turns out to be attached to, a file it names
                            // inside the workspace is a file worth reading.
                            let (hits, _) = resolve(&named, &blocks);
                            out.extend(hits);
                            if in_attr > 0 {
                                pending = Some(named);
                            }
                            i = after;
                            continue;
                        }
                        // Inside an attribute, `path =` is the module-redirect vocabulary, and one
                        // the guard cannot READ is the same hole the `include!` parser already
                        // refuses: an attribute may put its literal on the next line, or compute
                        // it. Outside an attribute it is an ordinary binding and means nothing.
                        None => assert!(
                            in_attr == 0,
                            "`path =` in {} is not followed by a plain string literal on the same \
                             line:\n  {}\nThis guard will not guess which file the module is \
                             redirected to. A `#[path]` names code in the operator's binary, so \
                             teach this function the shape deliberately rather than letting the \
                             file through unscanned.",
                            file.display(),
                            line.trim()
                        ),
                    }
                }
                i = end;
                continue;
            }
            if in_attr > 0 {
                // Every other word inside an attribute is the attribute's own vocabulary.
                i = end;
                continue;
            }
            match word {
                "mod" => {
                    let mut name_at = skip_ws(line, end);
                    // `mod r#gen;` — the raw-identifier prefix is not part of the name, and the
                    // name is the directory everything nested inside it resolves through.
                    if line.get(name_at..).is_some_and(|r| r.starts_with("r#")) {
                        name_at += 2;
                    }
                    let mut name_end = name_at;
                    while b
                        .get(name_end)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    {
                        name_end += 1;
                    }
                    if name_end > name_at {
                        opening = Some((code[name_at..name_end].to_owned(), pending.take()));
                    }
                    i = name_end;
                }
                // A visibility, and the `(crate)` / `(in path)` that may follow it, still has a
                // `mod` behind it — so it must not be read as the item the attribute landed on.
                "pub" => {
                    let mut j = skip_ws(line, end);
                    if b.get(j) == Some(&b'(') {
                        let mut nesting = 0usize;
                        while j < b.len() {
                            match b[j] {
                                b'(' => nesting += 1,
                                b')' => {
                                    nesting -= 1;
                                    if nesting == 0 {
                                        j += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            j += 1;
                        }
                    }
                    i = j;
                }
                // Any other item keyword: the attribute above it was not a module redirect.
                _ => {
                    pending = None;
                    i = end;
                }
            }
        }
    }
    out
}

/// The file an `include!` names, or a PANIC saying why the guard could not read the directive.
///
/// `at` is the byte offset just past the `!`, in a line whose blanked view told us this really is
/// the macro. Everything after that is read out of the ORIGINAL line, which `code_only` guarantees
/// is byte-for-byte the same length.
///
/// There is no lenient branch. `include!` names code and nothing else, so a shape this function
/// cannot read is a file the compiler pulls into the operator's binary that the guard did not
/// open — and "I could not look" must never come out as "I looked and it was clean".
fn strict_include_target(root: &Path, file: &Path, line: &str, at: usize) -> PathBuf {
    let mut i = skip_ws(line, at);
    assert!(
        line.as_bytes().get(i).is_some_and(|o| b"([{".contains(o)),
        "`include!` in {} does not open with a bracket and a string literal on the same line:\n  \
         {}\nThis guard will not guess what it expands to. A file the compiler pulls in is code \
         in the operator's binary, so teach this function the shape deliberately rather than \
         letting the file through unscanned.",
        file.display(),
        line.trim()
    );
    i = skip_ws(line, i + 1);
    assert!(
        line.as_bytes().get(i) == Some(&b'"'),
        "`include!` in {} names its target with something other than a plain string literal:\n  \
         {}\nThis guard will not guess what it expands to. A file the compiler pulls in is code \
         in the operator's binary, so teach this function the shape deliberately rather than \
         letting the file through unscanned.",
        file.display(),
        line.trim()
    );
    let body = i + 1;
    let len = line[body..].find('"').unwrap_or_else(|| {
        panic!(
            "unterminated path literal after `include!` in {}:\n  {}",
            file.display(),
            line.trim()
        )
    });
    let named = &line[body..body + len];
    // An `include!` is resolved against the directory of the file it is written in, whatever
    // inline module it sits in — unlike `#[path]`, which is why the two are resolved apart.
    let dir = file
        .parent()
        .expect("a scanned file has a parent directory");
    let target = normalise(&dir.join(named));
    assert!(
        target.starts_with(root),
        "`include!` in {} names `{named}`, which resolves OUTSIDE the workspace ({}). The guard \
         scans the workspace; it cannot vouch for a file it will never read.",
        file.display(),
        target.display()
    );
    assert!(
        target.is_file(),
        "`include!` in {} names `{named}`, and {} is not a file. Either the path is stale, or it \
         names a directory this guard does not know how to expand — both are for a human to \
         resolve, not for the guard to skip.",
        file.display(),
        target.display()
    );
    target
}

/// Every file the compiler can pull into a crate rooted at `from`: the `.rs` files under it, plus
/// everything a code-inclusion directive in one of them names, transitively.
///
/// This is the entry point BOTH textual rules and the git oracle use, so widening it widens all
/// three at once — which is the point. The walk used to be "collect `.rs`", and every check on it
/// filtered on `.rs` too, so the extension was an assumption no part of the guard could question.
fn scanned_sources(root: &Path, from: &Path) -> Vec<PathBuf> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut queue = rust_files(from);
    while let Some(file) = queue.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let src = fs::read_to_string(&file).unwrap_or_else(|e| {
            panic!(
                "the D3 write guard could not read {} ({e}). A guard that cannot see a file the \
                 compiler reads must not report green.",
                file.display()
            )
        });
        queue.extend(code_includes(root, &file, &src));
    }
    seen.into_iter().collect()
}

/// Workspace-relative, forward-slashed, for stable comparison against the exemption list.
fn rel(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .expect("every scanned file is under the workspace root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The `members = [...]` entries from the root `Cargo.toml`.
///
/// Hand-parsed rather than shelled out to `cargo metadata`: a nested cargo invocation inside
/// `cargo test` can block on the package-cache lock, and this guard runs in a pre-commit hook where
/// a hang is indistinguishable from a gate that passed. A glob member is a hard FAIL rather than a
/// best-effort expansion — deny by default extends to the parser too.
fn workspace_members(root: &Path) -> Vec<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .expect("the workspace root Cargo.toml is readable");
    // Anchored to a line whose first word is `members`, because a plain search for the word also
    // matches inside `default-members` — and a manifest that declares `default-members` first
    // handed this parser that shorter list, so rule 1 quietly stopped looking at every crate
    // missing from it while still reporting a clean scan.
    let mut found = None;
    let mut offset = 0usize;
    for line in manifest.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        if line
            .trim_start()
            .strip_prefix("members")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
        {
            found = Some(offset + indent);
            break;
        }
        offset += line.len();
    }
    let start = found.expect(
        "the workspace manifest has no line beginning `members =`. This guard will not fall back \
         to a looser search: the looser search is what made it read `default-members`.",
    );
    let open = manifest[start..]
        .find('[')
        .map(|i| start + i)
        .expect("`members` is a list");
    let close = manifest[open..]
        .find(']')
        .map(|i| open + i)
        .expect("the `members` list is closed");

    let mut members = Vec::new();
    let body = &manifest[open + 1..close];
    let mut rest = body;
    while let Some(a) = rest.find('"') {
        let after = &rest[a + 1..];
        let b = after.find('"').expect("an unterminated member string");
        members.push(after[..b].to_owned());
        rest = &after[b + 1..];
    }

    assert!(
        !members.is_empty(),
        "parsed ZERO workspace members out of Cargo.toml — the guard would scan nothing and pass"
    );
    for m in &members {
        assert!(
            !m.contains('*') && !m.contains('?'),
            "workspace member `{m}` is a glob. This guard refuses to guess what it expands to — \
             teach it the pattern deliberately rather than letting a crate slip through unscanned."
        );
        assert!(
            !m.starts_with("target/") && m != "target",
            "workspace member `{m}` lives under target/, which this guard does not walk"
        );
    }
    members
}

/// Strip the `GIT_*` variables that make a `git` command ask about a DIFFERENT repository than the
/// directory it was handed.
///
/// `GIT_DIR` and friends are exported into every hook git runs, and they WIN over the working
/// directory. Two things follow, and both bit. This guard's only second opinion stops being about
/// the tree it is guarding — inside a hook it asks whatever `GIT_DIR` names, and an oracle about
/// another repository agrees with anything. And the synthetic repository in
/// [`a_workspace_root_below_the_repository_root_still_gets_its_second_opinion`] stopped being
/// synthetic: its `git init` and `git add -A` landed on the REAL repository and replaced the index
/// that was about to be committed. Ask about the directory, always.
fn ask_about_this_directory(cmd: &mut std::process::Command) {
    for leaked in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
    ] {
        cmd.env_remove(leaked);
    }
}

/// A `git` invocation that asks about `dir` and nothing else. See [`ask_about_this_directory`].
fn git_in(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir);
    ask_about_this_directory(&mut cmd);
    cmd
}

/// The paths git holds in its index for `root`, or `None` if there is no index to ask.
///
/// This is the second opinion the guard has never had. Every hole found in it so far was the same
/// shape — the scan believed it had covered more than it had, and nothing outside the scan could
/// say otherwise. `git ls-files` is a source of truth this file had no hand in building: it is, by
/// definition, the set of files that ship.
///
/// **There is no `None`.** The first version returned one when `root/.git` did not exist, and the
/// two oracles answered that with an `eprintln!` and a `return` — which `cargo test` swallows for
/// a passing test, so the guard silently reverted to its pre-oracle strength and still printed
/// `ok`. It also mis-read every workspace that is a SUBDIRECTORY of its repository, where there is
/// no `.git` beside the root and the index is perfectly readable. So the question is put to git
/// itself rather than to a directory listing, and every failure — no repository, git not on PATH,
/// a non-zero exit, a non-UTF-8 path, an empty listing — is a hard FAIL. A second opinion the
/// guard could not obtain must never read as agreement.
///
/// `-z` is load-bearing: without it git quotes non-ASCII paths, and a path containing a newline
/// would split into two rows. Shelling to git is safe where shelling to cargo was not (see
/// [`workspace_members`]): this reads `.git/index`, takes no lock, and inside the pre-commit hook
/// it reads exactly the index that is about to be committed, which is the set we want.
fn git_tracked(root: &Path) -> Vec<String> {
    let out = git_in(root)
        .args(["ls-files", "-z", "--cached"])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "git could not be run in {} ({e}). This guard's only independent second opinion \
                 cannot be obtained, and a guard that stops checking must say so rather than \
                 pass. Fix the environment; do not soften this.",
                root.display()
            )
        });
    assert!(
        out.status.success(),
        "`git ls-files` failed in {}: {}\nThis is the cross-check that catches a file the scan \
         never opened, and it cannot run. If this tree is an export with no repository, it cannot \
         be verified here — build from the repository instead.",
        root.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let listing =
        String::from_utf8(out.stdout).expect("the tracked paths in this repository are UTF-8");
    let files: Vec<String> = listing
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    assert!(
        !files.is_empty(),
        "git tracks nothing in {} — the cross-check would hold the scan against an empty set and \
         agree with absolutely anything",
        root.display()
    );
    files
}

/// Where real CODE resumes on each line: 0, or the byte offset just past a string, raw string or
/// block comment that opened on an EARLIER line.
///
/// [`code_only`] reads one line at a time, which is safe for the call rule — an unclosed literal
/// only makes it blank more, and blanking more can only produce a false positive. It is NOT safe
/// for the two decisions that EXCUSE and that RESOLVE: the lines `#[cfg(test)]` and `mod tests {`
/// written inside a multi-line raw string are prose, and a span opened on them runs on through
/// real production code; a `{` inside one would move the inline module a `#[path]` is read
/// against. So both get the whole-file view that the line-at-a-time scanner cannot give them.
///
/// A line whose carry never closes yields its own length: there is no code on it at all.
fn code_resume_offsets(src: &str) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Carry {
        None,
        Str,
        Raw(usize),
        Comment(usize),
    }

    let mut out = Vec::new();
    let mut carry = Carry::None;
    for line in src.lines() {
        let started = carry != Carry::None;
        let mut resume: Option<usize> = None;
        let b = line.as_bytes();
        let mut i = 0;
        loop {
            match carry {
                Carry::Str => {
                    let mut closed = false;
                    while i < b.len() {
                        if b[i] == b'\\' {
                            i += 2;
                            continue;
                        }
                        if b[i] == b'"' {
                            i += 1;
                            closed = true;
                            break;
                        }
                        i += 1;
                    }
                    if !closed {
                        break;
                    }
                    if started && resume.is_none() {
                        resume = Some(i);
                    }
                    carry = Carry::None;
                }
                Carry::Raw(hashes) => {
                    let mut closed = false;
                    while i < b.len() {
                        if b[i] == b'"'
                            && b.get(i + 1..i + 1 + hashes)
                                .is_some_and(|t| t.iter().all(|c| *c == b'#'))
                        {
                            i += 1 + hashes;
                            closed = true;
                            break;
                        }
                        i += 1;
                    }
                    if !closed {
                        break;
                    }
                    if started && resume.is_none() {
                        resume = Some(i);
                    }
                    carry = Carry::None;
                }
                Carry::Comment(depth) => {
                    let mut depth = depth;
                    let mut closed = false;
                    while i + 1 < b.len() {
                        if b[i] == b'/' && b[i + 1] == b'*' {
                            depth += 1;
                            i += 2;
                            continue;
                        }
                        if b[i] == b'*' && b[i + 1] == b'/' {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                closed = true;
                                break;
                            }
                            continue;
                        }
                        i += 1;
                    }
                    if !closed {
                        carry = Carry::Comment(depth);
                        break;
                    }
                    if started && resume.is_none() {
                        resume = Some(i);
                    }
                    carry = Carry::None;
                }
                Carry::None => {
                    if i >= b.len() {
                        break;
                    }
                    // A char literal is consumed whole, for the same reason `code_only` does it:
                    // the `"` inside `'"'` is not a quote.
                    if b[i] == b'\''
                        && let Some(len) = char_literal_len(b, i)
                    {
                        i += len;
                        continue;
                    }
                    // A raw-string opener: `r"`, `r#"`, `br##"`, `cr#"`, … The body is then left
                    // to the Raw arm, which closes it on this line or carries it to the next.
                    if let Some(first) = raw_string_prefix(b, i) {
                        let mut j = first;
                        while b.get(j) == Some(&b'#') {
                            j += 1;
                        }
                        if b.get(j) == Some(&b'"') {
                            carry = Carry::Raw(j - first);
                            i = j + 1;
                            continue;
                        }
                    }
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        carry = Carry::Comment(1);
                        i += 2;
                        continue;
                    }
                    // A line comment ends the line; nothing after it can open anything.
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
                        break;
                    }
                    if b[i] == b'"' {
                        carry = Carry::Str;
                        i += 1;
                        continue;
                    }
                    i += 1;
                }
            }
        }
        out.push(if started {
            resume.unwrap_or(line.len())
        } else {
            0
        });
    }
    out
}

/// Every line of `src` with strings and comments blanked to spaces, WHOLE-FILE aware.
///
/// [`code_only`] alone starts every line in code mode, so the contents of a multi-line raw string
/// read to it as code. That is harmless where blanking less only costs a false positive; it is not
/// harmless for the two decisions that need to see the file the way rustc does — which lines a
/// `#[cfg(test)] mod` covers, and which inline module a `#[path]` sits inside.
///
/// Byte offsets are preserved line for line, so a hit still maps back to the original text.
fn code_lines(src: &str) -> Vec<String> {
    src.lines()
        .zip(code_resume_offsets(src))
        .map(|(line, at)| {
            if at == 0 {
                return code_only(line);
            }
            let tail = line.get(at..).unwrap_or_else(|| {
                panic!(
                    "the D3 write guard lost the boundary between text and code on this line:\n  \
                     {line}\nIt cannot tell which part is a string literal and which part is a \
                     call, and a guard that cannot read a line must not excuse it."
                )
            });
            let mut blanked = " ".repeat(at);
            blanked.push_str(&code_only(tail));
            blanked
        })
        .collect()
}

/// The 0-based line numbers that sit inside a `#[cfg(test)]` module.
///
/// This function decides what the call rule is allowed to SKIP, so every ambiguity in it has to
/// resolve towards excusing less. It has not, twice. First the closing line was found by an EXACT
/// match against `}` at the module's indentation, so a module that closed `} // tests` was never
/// found. Then the exact match became "the first line at that indentation whose CODE begins with
/// `}`" — and the INDENTATION was still the evidence. That only ever worked because `cargo fmt`
/// normalises indentation, and `#[rustfmt::skip]`, an ordinary one-line attribute, opts out of
/// exactly that. Hand-indent the closing brace by two spaces and the search ran past it to the
/// next brace that happened to line up, excusing every production line in between.
///
/// So indentation is no longer evidence at all. Four rules keep the ambiguity pointed the safe way:
///
/// 1. The close is found by COUNTING BRACES in the blanked text from the `mod` line — the same
///    thing the compiler counts. Whitespace decides nothing.
/// 2. If the braces do not balance, NOTHING is excused. Running to the end of the file was the
///    fail-open answer to "I could not find the end"; so was trusting a brace that merely lined up.
/// 3. The closing line itself is not excused, so anything written after the brace on that line is
///    still read as production code.
/// 4. A `#[cfg(test)]` that is only TEXT — inside a multi-line string or comment — opens nothing,
///    because [`code_lines`] has already blanked it away. It is the one place in this file that
///    needs a whole-file view rather than a line at a time, because it is the one place that
///    excuses code.
fn test_line_span(src: &str) -> Vec<bool> {
    let code = code_lines(src);
    let mut in_test = vec![false; code.len()];

    let mut i = 0;
    while i < code.len() {
        // Read from the BLANKED view: a `#[cfg(test)]` written inside a multi-line string is
        // prose, and there is nothing left of it here to open a span with.
        if code[i].trim() == "#[cfg(test)]" {
            // The item it applies to. Only a `mod` opens a span worth tracking; a `#[cfg(test)]`
            // on a single `use` or `fn` covers no call sites we would otherwise excuse.
            if let Some(open) = code.get(i + 1)
                && open.trim_start().starts_with("mod ")
                && open.trim_end().ends_with('{')
                && let Some(end) = block_close_line(&code, i + 1)
            {
                for flag in in_test.iter_mut().take(end).skip(i) {
                    *flag = true;
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    in_test
}

/// The line on which the block opening on line `open` closes, counted in braces, or `None` if the
/// braces never balance.
///
/// `None` is the fail-CLOSED answer and it is the important one: a module whose end this function
/// cannot find excuses nothing, so its own test bodies come out as offenders and someone reads the
/// failure. The alternative — guessing an end — is what excused live production code twice.
///
/// The text handed in is already blanked by [`code_lines`], so a `{` in a string, a comment, a
/// `'{'` char literal or a multi-line raw string is not a brace here, exactly as it is not one to
/// the compiler.
fn block_close_line(code: &[String], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (n, line) in code.iter().enumerate().skip(open) {
        for byte in line.bytes() {
            match byte {
                b'{' => depth += 1,
                // More closes than opens: we are not reading this block the way rustc does, and a
                // span we cannot account for is one we must not open.
                b'}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(n);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The line with every string-literal span and every trailing `//` comment blanked to spaces,
/// so a scan sees CODE and only code. Byte indices are preserved, so a hit still maps back to the
/// original text for the failure message.
///
/// `const METHOD: &'static str = "pane.send_text";` and
/// `tracing::warn!(… "pane.send_keys called with NO keys" …)` are the real lines this has to let
/// through: they name the method in data, and blanking string spans is what tells them apart from
/// a reference to the function.
///
/// EVERY OTHER TOKEN THAT CAN CONTAIN A `"` IS CONSUMED WHOLE, BEFORE THE PLAIN-STRING ARM, and
/// that ordering is the whole design. Read one quote at a time, each of these looked like a string
/// that opened where it did not, so the scanner stayed "inside a string" across the rest of the
/// line and a real `c.send_text(..)` after it became invisible:
///
/// * a `'"'` char literal, which a reviewer used to drive `pane.send_text` onto the wire through
///   the shipped binary with this suite green;
/// * a raw string — `r#"say "hi"#` closes on `"#`, and this file used to claim, wrongly, that raw
///   strings "blank correctly" because they open on their `"`;
/// * a `/* 6" of rope */` block comment, a kind of comment the scanner did not know existed.
///
/// All three are FAIL-OPEN, whatever they look like: blanking more code means the scanner sees
/// less, never more. Do not remove an arm; if a fourth token shape turns up, add one.
fn code_only(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                out.push(' ');
                if i + 1 < bytes.len() {
                    out.push(' ');
                    i += 1;
                }
                i += 1;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            out.push(' ');
            i += 1;
            continue;
        }
        // A char literal is consumed WHOLE, before the string arm can mistake its `"` for a
        // quote. `'a'`, `'\''`, `'\u{1b}'` are literals; `&'static` is a lifetime and is copied
        // through untouched (a lifetime has no closing `'`).
        if c == b'\'' {
            if let Some(len) = char_literal_len(bytes, i) {
                for _ in 0..len {
                    out.push(' ');
                }
                i += len;
                continue;
            }
        }
        // A raw string is a token, not a run of quotes.
        if let Some(len) = raw_string_len(bytes, i) {
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        // A block comment is prose that may contain a quote, and the plain-string arm would have
        // taken that quote for the start of a span.
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let len = block_comment_len(bytes, i);
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push(' ');
            i += 1;
            continue;
        }
        // A trailing comment is prose, not a call. Everything from here is blanked.
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while out.len() < line.len() {
                out.push(' ');
            }
            break;
        }
        // Multi-byte chars are copied whole so byte offsets stay aligned.
        let ch = line[i..].chars().next().expect("i is a char boundary");
        out.push_str(&line[i..i + ch.len_utf8()]);
        i += ch.len_utf8();
    }
    out
}

/// The byte just past a raw-string PREFIX at `bytes[start]`, or `None` if none starts there.
///
/// The three Rust accepts are `r`, `br` and `cr`. `cr` — the C raw string, stable since 1.77 and
/// legal in this edition-2024 workspace — was the one missing, and missing it was worse than not
/// knowing the spelling: the boundary rule here rejected the `r` in `cr#"` because a `c` sits in
/// front of it, so the opener was read as a bare quote and the literal tracked as a PLAIN string.
/// A body carrying an odd number of quotes then left the scanner inside a string for the rest of
/// the line — and, in the whole-file view, for the rest of the FILE, blanking away every code
/// inclusion after it. Ordinary modern Rust, and the guard quietly stops guarding.
///
/// `b"…"` and `c"…"` are deliberately absent: their quote opens and closes exactly like a plain
/// string's, which is what the callers already do with it. Only a RAW prefix changes the rules.
///
/// An `r` that is only the tail of an identifier (`for`, `char`) opens nothing — checked on the
/// byte before it, so a name is never mistaken for a prefix.
fn raw_string_prefix(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        return None;
    }
    match bytes[start] {
        b'r' => Some(start + 1),
        b'b' | b'c' if bytes.get(start + 1) == Some(&b'r') => Some(start + 2),
        _ => None,
    }
}

/// Length in bytes of the raw string literal starting at `bytes[start]`, or `None` if nothing
/// starts one there. Covers `r"…"`, `r#"…"#`, `r##"…"##` and the `br…` / `cr…` spellings, via
/// [`raw_string_prefix`].
///
/// An unterminated literal consumes the rest of the line. That is not a concession: if the closing
/// delimiter is not on this line then the rest of this line really is string content.
fn raw_string_len(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = raw_string_prefix(bytes, start)?;
    let first_hash = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    let hashes = i - first_hash;
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    // The closing delimiter is a quote followed by exactly as many `#` as the opener carried.
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'"'
            && bytes
                .get(j + 1..j + 1 + hashes)
                .is_some_and(|tail| tail.iter().all(|b| *b == b'#'))
        {
            return Some(j + 1 + hashes - start);
        }
        j += 1;
    }
    Some(bytes.len() - start)
}

/// Length in bytes of the `/* … */` comment starting at `bytes[start]`, nesting counted the way
/// rustc counts it. An unclosed comment consumes the rest of the line, which is what it is.
fn block_comment_len(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
            continue;
        }
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i - start;
            }
            continue;
        }
        i += 1;
    }
    bytes.len() - start
}

/// Length in bytes of the Rust char literal starting at `bytes[start]` (a `'`), or `None` if this
/// `'` opens a lifetime instead. Deliberately conservative: anything it cannot positively identify
/// as a literal is left for the other arms, so an unrecognised form never silently blanks code.
fn char_literal_len(bytes: &[u8], start: usize) -> Option<usize> {
    debug_assert_eq!(bytes[start], b'\'');
    let body = start + 1;
    if body >= bytes.len() {
        return None;
    }
    if bytes[body] == b'\\' {
        // Escaped: scan to the closing quote. Bounded, so a stray backslash cannot run away.
        let mut j = body + 2;
        let limit = (start + 12).min(bytes.len());
        while j < limit {
            if bytes[j] == b'\'' {
                return Some(j - start + 1);
            }
            j += 1;
        }
        return None;
    }
    // Unescaped: exactly one char, which may be multi-byte, then the closing quote.
    let ch_len = match std::str::from_utf8(&bytes[body..]) {
        Ok(rest) => rest.chars().next()?.len_utf8(),
        Err(e) if e.valid_up_to() > 0 => 1,
        Err(_) => return None,
    };
    let close = body + ch_len;
    if bytes.get(close) == Some(&b'\'') {
        Some(close - start + 1)
    } else {
        None
    }
}

/// Does `line` REACH one of the three?
///
/// Two shapes count, because both hand a caller the real function:
///
/// * a **call** — the name followed by `(`: `client.send_text(`, `HerdrClient::send_text(` (the
///   UFCS form the old `.send_text(`-only rule missed), `<T>::send_text(`, a bare `send_text(`;
/// * a **reference** — the name NOT followed by `(`: `let f = HerdrClient::send_text;` then
///   `f(c, p, "…")`. A `(`-anchored rule cannot see that, and a wrapper inside the client crate
///   with an innocuous name would carry it all the way out to the binary.
///
/// Excused, and only these: the `fn send_text(` definition itself, a longer identifier that merely
/// ends in the name (`do_send_text(`), and anything inside a string literal or a comment — all
/// blanked by [`code_only`] before we look.
///
/// Returns the offending method name, or `None`.
fn write_reach_on(line: &str) -> Option<&'static str> {
    let code = code_only(line);
    for name in WRITE_NAMES {
        let mut from = 0;
        while let Some(off) = code[from..].find(name) {
            let start = from + off;
            let end = start + name.len();
            from = end;

            // Part of a longer identifier — `do_send_text(` is a different function, and
            // `send_text_and_send_keys_…` is a test name.
            let bounded_left = !code[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let bounded_right = !code[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !bounded_left || !bounded_right {
                continue;
            }
            // `pub async fn send_text(` — the definition, which lives in this crate by design.
            if code[..start].trim_end().ends_with("fn") {
                continue;
            }
            return Some(name);
        }
    }
    None
}

/// Rule 1's offenders in the workspace rooted at `root`, and how many files it read to find them.
///
/// Scoped by the DEFINING crate and walked from the ROOT — not `crates/herdr-tg/src`, and no
/// longer a list of member directories. A crate added in slice 2 is covered on the commit that
/// adds it, and so is a member's own source file that a Cargo target path parks outside the
/// member's directory. Comments and string literals count: a `//` TODO naming the method is one
/// session away from being a call.
///
/// Split out of the test so the rule can be pointed at a synthetic tree that reproduces a shape
/// this repository does not currently have — see
/// [`a_source_file_outside_every_member_directory_still_may_not_name_a_write_method`].
fn names_a_write_outside_the_defining_crate(root: &Path) -> (usize, Vec<String>) {
    let members = workspace_members(root);
    for member in &members {
        let dir = root.join(member);
        assert!(
            dir.is_dir(),
            "workspace member `{member}` is declared in Cargo.toml but {} does not exist — the \
             guard cannot scan it and will not pretend it did",
            dir.display()
        );
    }

    // From the ROOT, not from each member directory in turn. A member's own source file does not
    // have to live inside it — a Cargo target path (`[[bin]] path = "../../tools/relay.rs"`) puts
    // real crate source anywhere in the tree — and this rule is the one that catches a MENTION,
    // which is what such a file was free to carry while the walk started at the member directory.
    let all = scanned_sources(root, root);
    let scanned: BTreeSet<String> = all.iter().map(|f| rel(root, f)).collect();
    for member in members.iter().filter(|m| m.as_str() != DEFINING_MEMBER) {
        let prefix = format!("{member}/");
        assert!(
            scanned.iter().any(|f| f.starts_with(&prefix)),
            "workspace member `{member}` contributed ZERO scanned files. A vacuous scan is not a \
             pass."
        );
    }

    // The DEFINING crate is excused by where its code lives, which is the one thing this rule can
    // still say without reading a manifest. Everything else in the tree is a caller.
    let defining = format!("{DEFINING_MEMBER}/");
    let mut offenders = Vec::new();
    let mut checked = 0usize;
    for file in &all {
        let relp = rel(root, file);
        if relp.starts_with(&defining) {
            continue;
        }
        checked += 1;
        let src = fs::read_to_string(file).expect("a source file in this repo is readable");
        for (n, line) in src.lines().enumerate() {
            for name in WRITE_NAMES {
                if line.contains(name) && relp != AUDITED_WRITE_PATH {
                    offenders.push(format!("{relp}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }

    (checked, offenders)
}

/// Rule 1 — no file outside the defining crate may NAME a write method, anywhere in the tree.
#[test]
fn no_member_outside_the_client_crate_may_even_name_a_write_method() {
    let root = workspace_root();
    let (checked, offenders) = names_a_write_outside_the_defining_crate(&root);

    assert!(
        checked > 0,
        "scanned zero files across every non-client workspace member — the guard is vacuous"
    );
    assert!(
        offenders.is_empty(),
        "a workspace member outside `{DEFINING_MEMBER}` names a write method. These crates are \
         what a timer, a cron job or a Telegram message can reach, so a write may appear in \
         exactly ONE of their files: `{AUDITED_WRITE_PATH}`, which reads the pane back and audits. \
         If this is a new legitimate write path, it belongs in that module, not beside it. \
         Found:\n  {}",
        offenders.join("\n  ")
    );
}

/// Rule 2 — a *call* to one of the three may appear nowhere in the workspace outside
/// `#[cfg(test)]`, whatever crate, whatever target directory, whatever spelling.
#[test]
fn no_write_call_site_anywhere_outside_cfg_test() {
    let root = workspace_root();
    // Prove the exemptions still point at real files before trusting them.
    for exempt in CALL_RULE_EXEMPT {
        assert!(
            root.join(exempt).is_file(),
            "call-rule exemption `{exempt}` no longer exists. Delete the stale entry — a phantom \
             exemption is how a list stops describing the tree it is guarding."
        );
    }

    // Scanning from the ROOT, not from a list of crate directories: a `.rs` file that is not in any
    // member yet (a half-added crate, a stray example) is still a file someone can `cargo run`.
    let all = scanned_sources(&root, &root);
    assert!(
        all.len() > 10,
        "only {} .rs files found under {} — that is not this workspace; the walk is broken",
        all.len(),
        root.display()
    );

    // Every declared member must have contributed. Catches a renamed crate making the scan vacuous.
    let scanned: BTreeSet<String> = all.iter().map(|f| rel(&root, f)).collect();
    for member in workspace_members(&root) {
        let prefix = format!("{member}/");
        assert!(
            scanned.iter().any(|f| f.starts_with(&prefix)),
            "workspace member `{member}` contributed no scanned file. The guard looked everywhere \
             except the crate that was declared — that is a vacuous pass, not a clean one."
        );
    }

    let mut offenders = Vec::new();
    for file in &all {
        let relp = rel(&root, file);
        if CALL_RULE_EXEMPT.contains(&relp.as_str()) || relp == AUDITED_WRITE_PATH {
            continue;
        }
        let src = fs::read_to_string(file).expect("a source file in this repo is readable");
        let in_test = test_line_span(&src);
        for (n, line) in src.lines().enumerate() {
            if in_test[n] {
                continue;
            }
            // A doc comment naming the hazard is the opposite of a call site.
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if let Some(name) = write_reach_on(line) {
                offenders.push(format!("{}:{}: [{}] {}", relp, n + 1, name, code));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a write method is called from code that is not `#[cfg(test)]`. These three RPCs type real \
         keystrokes into the operator's real terminals; slice 1 has no live call site, by design \
         (D3). If this is deliberate, it is a decision for the operator, not a test to delete. \
         If a path below is not a file git tracks, it is scratch that landed in-tree (see the \
         `%h/` note in .gitignore) — delete it, and do NOT add it to the skip list: a scan that \
         skips whatever .gitignore hides turns .gitignore into a one-line bypass for this guard, \
         and the binary the operator runs is built from the working tree, ignored files included. \
         Found:\n  {}",
        offenders.join("\n  ")
    );
}

/// Rule 3 — `Request` must stay SEALED.
///
/// Rules 1 and 2 are textual, and text is exactly what a hostile or careless `const METHOD` can
/// evade: `concat!("pane.send", "_text")` reaches `pane.send_text` on the wire without either rule
/// ever seeing the string. What stops that is the compiler — `Request: sealed::Sealed` makes the
/// trait unimplementable outside this crate, so `HerdrClient::call` cannot be handed a foreign
/// method name at all. This test is the tripwire on that declaration; the enforcement is `rustc`.
#[test]
fn the_request_trait_is_sealed_so_no_foreign_crate_can_choose_a_method() {
    let client = workspace_root().join("crates/herdr-client/src/client.rs");
    let src = fs::read_to_string(&client).expect("client.rs is readable");

    assert!(
        src.contains("pub trait Request: sealed::Sealed"),
        "`Request` lost its `sealed::Sealed` supertrait in {}. Unsealed, it is a public generic \
         escape hatch onto the ENTIRE herdr method surface: any downstream crate implements it and \
         drives `pane.read {{source:\"recent\"}}` or `pane.send_text` through the public `call()`, \
         invisible to every textual guard in this file. Re-sealing is the fix; widening the guard \
         is not.",
        client.display()
    );
    // The seal only holds while the module stays private — `pub mod sealed` re-opens it silently.
    assert!(
        src.contains("\nmod sealed {"),
        "the `sealed` module in client.rs is no longer a plain private `mod sealed {{`. If it were \
         made `pub`, `Request` would be implementable downstream again and the seal would be \
         decorative."
    );
    // One `Sealed` impl per `Request` impl, or a request type silently stopped being usable /
    // a new one arrived unsealed.
    let requests = src.matches("impl Request for ").count();
    let seals = src.matches("impl sealed::Sealed for ").count();
    assert_eq!(
        requests, seals,
        "client.rs has {requests} `impl Request` but {seals} `impl sealed::Sealed` — every request \
         type gets exactly one of each, right above it."
    );
    assert!(
        requests >= 9,
        "expected the crate's 9 request types, found {requests}"
    );
}

/// The guard must be able to FAIL. A test that can only pass proves nothing, and this one's whole
/// value is that it fires on a future commit nobody is reviewing at 2 a.m.
///
/// Covers both spellings the reviewer got past the old version: the plain method call and the UFCS
/// form, plus the `fn` definition and the longer-identifier case that must NOT fire.
#[test]
fn the_guard_itself_detects_a_planted_call_site() {
    let planted = "\
fn relay(client: &HerdrClient, pane: &PaneId) {
    client.send_text(pane, \"oops\");
}

async fn evil_ufcs(c: &HerdrClient, p: &PaneId) {
    let _ = HerdrClient::send_text(c, p, \"rm -rf /\").await;
}

pub async fn send_keys(&self, pane: &PaneId) {}
fn do_send_input_wrapper() {}

fn evil_fn_pointer() {
    let f = HerdrClient::send_input;
}

const METHOD: &str = \"pane.send_text\";  // data, not a reference — must be excused

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn fine_here() {
        client.send_keys(&pane, &keys).await.unwrap();
    }
}
";
    let in_test = test_line_span(planted);
    let mut flagged = Vec::new();
    for (n, line) in planted.lines().enumerate() {
        if in_test[n] || line.trim_start().starts_with("//") {
            continue;
        }
        if write_reach_on(line).is_some() {
            flagged.push(n + 1);
        }
    }
    assert_eq!(
        flagged,
        vec![2, 6, 13],
        "the three REACHABLE shapes must be caught — line 2 (method call), line 6 (UFCS), line 13 \
         (a bare fn-POINTER reference, which has no `(` after the name and which a call-only rule \
         cannot see). Five shapes must be excused: line 9 is a `fn` DEFINITION, line 10 is a \
         longer identifier, line 16 is the method name inside a STRING LITERAL, line 16's trailing \
         `//` comment, and the `#[cfg(test)]` block at the end. Got {flagged:?}"
    );
}

/// Rule 4 — `ReadSource` must stay `pub(crate)`, and the only two reads must stay pinned.
///
/// **Why this is here and not just a comment.** A reviewer found `cargo doc` red under
/// `RUSTDOCFLAGS='-D warnings'` with two `private_intra_doc_links` errors, both from public docs
/// linking to the deliberately-private `ReadSource`, and wrote the warning explicitly: *do not
/// resolve it by widening `ReadSource` to `pub`* — that is the one change that would undo the
/// compiler-enforced no-`recent` property. The links were downgraded and `cargo doc` became a
/// pre-commit gate.
///
/// Then the wrong fix was measured rather than assumed, and it WORKS: re-link `ReadSource` from
/// client.rs's public module docs and flip `pub(crate) enum ReadSource` to `pub enum ReadSource`,
/// and `cargo doc` goes green, `cargo fmt` green, `cargo clippy -D warnings` green, and
/// `cargo test --workspace` green. Every gate this repo runs accepts it. `tests/wire.rs`'s
/// `read_visible_sends_source_visible_and_omits_lines` does not catch it either: it asserts what
/// the two existing public constructors put on the wire, which is still `visible` — the hole is
/// that a *new* one could now be written.
///
/// `recent` / `recent_unwrapped` physically scroll the operator's real viewport when
/// `lines > viewport_rows`. The reason no timer can ever reach one is that the enum naming them is
/// unnameable outside this crate. That is a one-keyword property with no other enforcement, so it
/// gets a tripwire.
#[test]
fn the_read_source_enum_stays_crate_private_and_the_reads_stay_pinned() {
    let model = workspace_root().join("crates/herdr-client/src/proto/model.rs");
    let src = fs::read_to_string(&model).expect("model.rs is readable");

    assert!(
        src.contains("pub(crate) enum ReadSource"),
        "`ReadSource` is no longer `pub(crate)` in {}. It names `recent` and `recent_unwrapped`, \
         which harvest-scroll the operator's REAL viewport when `lines > viewport_rows` — the \
         screen physically moves while they are working. Its crate-privacy is the whole reason no \
         caller outside this crate can choose a read source, and it is enforced by nothing but \
         that keyword. If you arrived here from a `private_intra_doc_links` error, the fix is to \
         un-link the doc reference (write `ReadSource` in plain backticks), NEVER to widen this.",
        model.display()
    );
    assert!(
        !src.contains("pub enum ReadSource"),
        "found a `pub enum ReadSource` declaration in {}",
        model.display()
    );

    // The two pinned constructors are the other half: a private enum with a public constructor
    // that accepts one is the same hole wearing a different hat.
    let client = workspace_root().join("crates/herdr-client/src/client.rs");
    let csrc = fs::read_to_string(&client).expect("client.rs is readable");
    for pinned in [
        "pub async fn read_visible(",
        "pub async fn read_visible_tail(",
    ] {
        assert!(
            csrc.contains(pinned),
            "`{pinned}` is gone from {}. The two visible-pinned reads are the entire public read \
             surface; if one was renamed, re-pin this guard to the new name rather than deleting \
             the row.",
            client.display()
        );
    }
    // No public signature may take a ReadSource — that would hand the choice to the caller.
    for line in csrc.lines() {
        let l = line.trim_start();
        if l.starts_with("pub fn ") || l.starts_with("pub async fn ") {
            assert!(
                !l.contains("ReadSource"),
                "a PUBLIC method takes a `ReadSource`, which lets a caller choose a \
                 harvest-scrolling source: {l}"
            );
        }
    }
}

/// Regression: a `'"'` char literal must not open a string span.
///
/// Round-two review drove `pane.send_text` onto the wire through the shipped binary with this
/// suite green, because `code_only` read the `"` inside `'"'` as a quote and blanked the rest of
/// the line — hiding the call that followed it. Blanking MORE hides code from the scanner, so
/// this failure mode is fail-open. Each case below was red before the char-literal arm landed.
#[test]
fn char_literals_do_not_open_a_string_span() {
    // The exact shape the reviewer used to smuggle a live call past the guard.
    let smuggle = r#"    let q = '"'; c.send_text(&pane, "rm -rf ~\n").await?;"#;
    assert!(
        code_only(smuggle).contains("send_text("),
        "a call after a '\"' char literal must stay visible to the guard, got: {:?}",
        code_only(smuggle)
    );

    // An escaped char literal must not swallow the call either.
    let escaped = r#"    let nl = '\n'; c.send_keys(&pane, &keys).await?;"#;
    assert!(code_only(escaped).contains("send_keys("));

    // A lifetime is not a char literal and must survive untouched.
    let lifetime = r#"    const METHOD: &'static str = "pane.send_text";"#;
    let seen = code_only(lifetime);
    assert!(seen.contains("&'static"), "lifetime mangled: {seen:?}");
    assert!(
        !seen.contains("pane.send_text"),
        "the method NAME in a string is data, not a call site: {seen:?}"
    );

    // The property that makes the whole scanner sound: blanking preserves byte offsets.
    for case in [smuggle, escaped, lifetime] {
        assert_eq!(
            code_only(case).len(),
            case.len(),
            "offset drift on {case:?}"
        );
    }
}

/// The audited write path must EXIST, be exactly one file, and actually audit.
///
/// An exemption is only as good as the thing it points at. Three ways this one could rot into a
/// hole, each checked here:
///
/// 1. The file is renamed or deleted. The exemption then matches nothing, the guard silently goes
///    back to "nowhere", and the next write lands somewhere else entirely.
/// 2. The file stops verifying. If `deliver.rs` ever reports success on an ack alone, the exemption
///    is buying a guarantee that is no longer delivered — the whole reason writes are allowed there
///    and not elsewhere.
/// 3. A second file is added to the list. That is a decision about the operator's terminals, and it
///    should require editing this assertion, not just appending a string.
#[test]
fn the_audited_write_path_exists_and_still_earns_its_exemption() {
    let root = workspace_root();
    let path = root.join(AUDITED_WRITE_PATH);
    assert!(
        path.is_file(),
        "AUDITED_WRITE_PATH points at {}, which does not exist. A dangling exemption is worse \
         than none: it looks like a guarantee and enforces nothing.",
        path.display()
    );

    let src = fs::read_to_string(&path).expect("the audited write path is readable");

    // It must read the pane back. That is what distinguishes this module from any other caller and
    // is the entire justification for letting writes live here.
    assert!(
        src.contains("read_visible"),
        "{AUDITED_WRITE_PATH} no longer reads the pane back. Writes are permitted here ONLY \
         because delivery is verified rather than assumed."
    );

    // It must not be able to claim delivery from an ack. The rung vocabulary is what keeps the
    // operator's confirmation honest.
    assert!(
        src.contains("Rung::Acted"),
        "{AUDITED_WRITE_PATH} no longer distinguishes what it observed from what it hopes. \
         An `ok` from herdr means herdr took the bytes, never that the agent acted."
    );

    // Exactly one exemption. Widening this is a deliberate act.
    assert_eq!(
        AUDITED_WRITE_PATH, "crates/herdr-tg/src/deliver.rs",
        "the audited write path moved. That is allowed — but update this assertion deliberately, \
         because it is the only thing standing between a Telegram message and a keystroke."
    );
}

/// The sibling of [`the_guard_itself_detects_a_planted_call_site`]: that one proves the LEXER can
/// fire, this proves the WALK can reach the line to hand it.
///
/// `target` is an ordinary module name — this crate already has a `Target` enum in routing.rs — and
/// a skip written on the bare directory name pruned `crates/<crate>/src/target/` out of the tree
/// before either textual rule ever saw it. A live, reachable `send_text` sat there with all seven
/// tests green. The tree here is SYNTHETIC and lives in a tempdir on purpose: a plant inside this
/// repository would be found by `no_write_call_site_anywhere_outside_cfg_test`, which walks the
/// workspace root, and would turn an unrelated test red for a reason no one could follow.
#[test]
fn the_walk_sees_a_source_module_named_target_and_still_skips_build_dirs() {
    let tmp = tempfile::tempdir().expect("a tempdir to build the synthetic workspace in");
    let root = tmp.path();
    let mk = |p: &str| {
        fs::create_dir_all(root.join(p)).expect("the synthetic workspace directories are creatable")
    };
    let put = |p: &str, body: &str| {
        fs::write(root.join(p), body).expect("the synthetic workspace files are writable")
    };

    mk("crates/demo/src/target");
    mk("target/debug/build");
    mk("crates/demo/target/debug/build");
    put("Cargo.toml", "[workspace]\nmembers = [\"crates/demo\"]\n");
    put("crates/demo/Cargo.toml", "[package]\nname = \"demo\"\n");
    put("crates/demo/src/lib.rs", "pub mod target;\n");
    // A source module that merely happens to be called `target`, with a live write in it.
    let plant = "fn relay(c: &C, p: &P) { c.send_text(p, \"oops\"); }\n";
    put("crates/demo/src/target/mod.rs", plant);
    // Real build output, above and beside a manifest. Both must stay unwalked.
    put(
        "target/debug/build/out.rs",
        "// generated by a build script\n",
    );
    put(
        "crates/demo/target/debug/build/out.rs",
        "// generated by a build script\n",
    );

    let seen: BTreeSet<String> = rust_files(root).iter().map(|f| rel(root, f)).collect();

    assert!(
        seen.contains("crates/demo/src/target/mod.rs"),
        "the walk skipped a SOURCE module because it is spelled `target`. Everything under it is \
         invisible to both textual rules, so a write into the operator's terminals can live there \
         with this suite green. Saw: {seen:?}"
    );
    assert!(
        !seen.contains("target/debug/build/out.rs"),
        "the workspace build directory is being walked; it holds generated code no human wrote"
    );
    assert!(
        !seen.contains("crates/demo/target/debug/build/out.rs"),
        "a member crate's own build directory is being walked"
    );
    assert_eq!(
        write_reach_on(plant.lines().next().expect("the plant is one line")),
        Some("send_text"),
        "the planted line is one the scanner already knows how to catch — reaching it was the only \
         thing ever missing"
    );
}

/// The scan's own coverage, held against a set it did not compute.
///
/// This is the general form of the bug that has now been fixed four times in this file. Each round
/// fixed one way of looking at too little — two hardcoded directories, an unverified member list,
/// a bare-name `target` skip, a `.rs` extension filter — and the first three rounds left the class
/// open, because every check the guard had was computed from the same walk it was supposed to be
/// checking. A guard that cannot tell you it scanned nothing is not a guard.
///
/// Round four is why this test has two clauses instead of one. The oracle DID exist, and a
/// reviewer walked straight past it anyway: it filtered the git index down to `.rs`, exactly like
/// the walk it was auditing, so a tracked `relay.inc` with a live `send_text` in it — one
/// `include!` line from the shipped binary — was invisible to the walk AND to its own second
/// opinion. **An oracle that shares its subject's blind spot is not an oracle.** So:
///
/// 1. every tracked `.rs` must be scanned, whatever the walk thinks of the directory it is in; and
/// 2. **extension-blind**: any other tracked file inside a workspace member that nothing scanned
///    must not REACH a write method. `.rs` gets no special standing here — a file is judged by
///    whether the guard read it and by what is in it.
///
/// Clause 2 stops at member directories because that is where crate sources live, and files
/// further out are covered from the other side ONLY: [`scanned_sources`] FOLLOWS a code inclusion
/// wherever it points, and [`code_includes`] hard-fails on an inclusion it cannot resolve. Read
/// that as the load-bearing claim it is. A reviewer put a live `send_text` in a tracked
/// `shared/relay.inc` — outside every member, so clause 2 skipped it; not `.rs`, so clause 1
/// skipped it — and reached it with a `#[path]` the guard resolved against the wrong directory.
/// Nothing here contradicted that, because this oracle cannot: a file outside every member is
/// reachable to it only through the resolution in [`code_includes`]. That resolution now follows
/// rustc's rules and fails loudly when it comes up empty, and that is the whole of what stands
/// between this clause and a third way for a file to become code.
///
/// The `.rs` direction is one-way on purpose: every tracked `.rs` must be scanned, but the scan
/// may hold more. Scanning extra files has never been the failure; a shipped file nobody opened is.
///
/// Tracked paths are filtered to ones that exist on disk, so a file staged for deletion is not
/// blamed on the walk. That is not a way out for a skipped directory — its files are all still
/// sitting there.
#[test]
fn every_file_that_ships_is_scanned_or_carries_no_write() {
    let root = workspace_root();
    let on_disk: Vec<String> = git_tracked(&root)
        .into_iter()
        .filter(|p| root.join(p).is_file())
        .collect();
    let scanned: BTreeSet<String> = scanned_sources(&root, &root)
        .iter()
        .map(|f| rel(&root, f))
        .collect();

    let ships: Vec<&String> = on_disk.iter().filter(|p| p.ends_with(".rs")).collect();
    assert!(
        ships.len() > 10,
        "only {} tracked .rs files under {} — an oracle that finds almost nothing agrees with \
         almost anything, which is a broken oracle rather than a pass",
        ships.len(),
        root.display()
    );

    let missed: Vec<&str> = ships
        .iter()
        .filter(|p| !scanned.contains(**p))
        .map(|p| p.as_str())
        .collect();
    assert!(
        missed.is_empty(),
        "{} of {} git-tracked .rs files were never opened by the scan. These files SHIP, and this \
         guard did not read a line of them — a write into the operator's real terminals can sit in \
         any of them with the whole suite green. Fix the WALK. Do not add an exemption for them, \
         and do not relax this assertion. Never opened:\n  {}",
        missed.len(),
        ships.len(),
        missed.join("\n  ")
    );

    // Clause 2. Nothing here looks at an extension.
    let members = workspace_members(&root);
    let mut unread_reach = Vec::new();
    let mut unread = 0usize;
    for path in &on_disk {
        if scanned.contains(path) || !members.iter().any(|m| path.starts_with(&format!("{m}/"))) {
            continue;
        }
        unread += 1;
        // Lossy on purpose: a fixture full of terminal escapes is still a file to look through,
        // and refusing to decode one would be a way to stop looking.
        let bytes = fs::read(root.join(path)).expect("a tracked file in this repo is readable");
        for (n, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            if let Some(name) = write_reach_on(line) {
                unread_reach.push(format!("{path}:{}: [{name}] {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        unread_reach.is_empty(),
        "{unread} tracked files inside a workspace member were never opened by the scan, and {} \
         of their lines REACH a write method. The extension is not the question — whether the \
         guard read the file is. If the compiler pulls this file in, make the scan follow it \
         (see `code_includes`); if it does not, the write reference does not belong there. \
         Found:\n  {}",
        unread_reach.len(),
        unread_reach.join("\n  ")
    );
}

/// The other input rule 1 trusts: the list of crates it is going to look at.
///
/// Rule 1 only ever examines declared workspace members, so a crate the manifest parser fails to
/// see is a crate that may name a write method as freely as it likes. Same shape as the walk's
/// blind spot, different input — which is why it gets the same treatment: hold the parsed list
/// against the crates git actually carries, rather than against itself.
#[test]
fn every_crate_that_ships_is_a_declared_workspace_member() {
    let root = workspace_root();
    let tracked = git_tracked(&root);

    let declared: BTreeSet<String> = workspace_members(&root).into_iter().collect();
    // The root manifest has no `/` before it and drops out of this on its own.
    let shipped: BTreeSet<String> = tracked
        .iter()
        .filter_map(|p| p.strip_suffix("/Cargo.toml"))
        .map(str::to_owned)
        .collect();

    let undeclared: Vec<&str> = shipped.difference(&declared).map(String::as_str).collect();
    assert!(
        undeclared.is_empty(),
        "these crates ship but are not in the member list this guard scans, so rule 1 never looks \
         at them: {undeclared:?}"
    );

    let phantom: Vec<&str> = declared.difference(&shipped).map(String::as_str).collect();
    assert!(
        phantom.is_empty(),
        "the member list names crates that ship no Cargo.toml: {phantom:?}. Either a crate moved \
         and the scan is now looking at nothing, or the manifest parse picked up the wrong list."
    );
}

/// Regression: a raw string or a block comment must not blank the code that follows it.
///
/// The same fail-open as the `'"'` char literal, twice more. `code_only` understood one kind of
/// quote and one kind of comment, so `r#"say "hi"#` left it convinced it was still inside a string
/// — and everything up to the next quote, including a real call, was blanked out of the scanner's
/// view. `/* 6" of rope */` did it with a quote inside a comment the scanner did not know existed.
/// Blanking MORE means the scanner sees LESS, which is the dangerous direction.
#[test]
fn raw_strings_and_block_comments_do_not_hide_a_call_from_the_scanner() {
    // Both shapes a reviewer used, verbatim.
    let raw = r##"    let doc = r#"say "hi"#; c.send_text(&pane, "rm -rf ~\n").await?;"##;
    assert!(
        code_only(raw).contains("send_text("),
        "a call after a raw string must stay visible, got: {:?}",
        code_only(raw)
    );
    let block = r#"    /* 6" of rope */ c.send_keys(&pane, &keys).await?; let z = "";"#;
    assert!(
        code_only(block).contains("send_keys("),
        "a call after a block comment must stay visible, got: {:?}",
        code_only(block)
    );

    // The method name INSIDE either one is data, not a call site — the property that made string
    // blanking worth having in the first place.
    let named = r####"    const M: &str = r#"pane.send_text"#; /* and send_keys too */"####;
    assert_eq!(
        write_reach_on(named),
        None,
        "the method name in a raw string or a comment is data: {:?}",
        code_only(named)
    );

    // A byte-string raw literal is the same token with one more prefix letter.
    let byte_raw = r##"    let b = br#"send_input"#; c.send_input(&pane, "x").await?;"##;
    assert_eq!(write_reach_on(byte_raw), Some("send_input"));

    // An `r` that is only the tail of an identifier does not open anything.
    let ident = r#"    for pane in panes { c.send_text(&pane, "x"); }"#;
    assert_eq!(write_reach_on(ident), Some("send_text"));

    // The property that makes the whole scanner sound: blanking preserves byte offsets.
    for case in [raw, block, named, byte_raw, ident] {
        assert_eq!(
            code_only(case).len(),
            case.len(),
            "offset drift on {case:?}"
        );
    }
}

/// Regression: `default-members` is not `members`, and confusing the two shrinks rule 1's world.
///
/// The list was located with `find("members")`, which matches inside `default-members` as happily
/// as it matches the real key. A manifest that declares `default-members` first therefore handed
/// rule 1 a shorter list, and every crate missing from it was free to name a write method — with
/// the guard reporting a clean scan of the crates it had been told about.
#[test]
fn a_manifest_that_declares_default_members_first_still_finds_the_real_member_list() {
    let tmp = tempfile::tempdir().expect("a tempdir to hold the synthetic manifest");
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\ndefault-members = [\"crates/one\"]\nmembers  = \
         [\"crates/one\", \"crates/two\"]\n",
    )
    .expect("the synthetic manifest is writable");

    assert_eq!(
        workspace_members(tmp.path()),
        vec!["crates/one".to_owned(), "crates/two".to_owned()],
        "the parser picked up `default-members` instead of `members`, so rule 1 would scan a \
         shorter list than the workspace actually ships"
    );
}

/// Regression: a file the compiler pulls in is scanned like the `.rs` file that names it.
///
/// The walk collected `.rs` files, and BOTH git oracles filtered the index down to `.rs` too — so
/// the oracle shared the exact assumption it existed to contradict. A tracked `relay.inc` holding
/// a live `send_text`, pulled into the shipped binary by a one-line `include!`, was invisible to
/// every rule and to both oracles, with the word `send_text` sitting in plain sight in a committed
/// file and all five gates green.
#[test]
fn a_file_pulled_in_by_include_or_path_is_scanned_like_the_rs_file_that_names_it() {
    let tmp = tempfile::tempdir().expect("a tempdir to build the synthetic workspace in");
    let root = tmp.path();
    fs::create_dir_all(root.join("crates/demo/src")).expect("the synthetic tree is creatable");
    let put = |p: &str, body: &str| {
        fs::write(root.join(p), body).expect("the synthetic workspace files are writable")
    };
    put("Cargo.toml", "[workspace]\nmembers = [\"crates/demo\"]\n");
    put("crates/demo/Cargo.toml", "[package]\nname = \"demo\"\n");
    put(
        "crates/demo/src/lib.rs",
        "pub mod relay;\n#[path = \"redirected.txt\"]\npub mod redirected;\n",
    );
    put("crates/demo/src/relay.rs", "include!(\"relay.inc\");\n");
    let plant = "pub async fn relay(c: &C, p: &P) { let _ = c.send_text(p, \"oops\").await; }\n";
    put("crates/demo/src/relay.inc", plant);
    put(
        "crates/demo/src/redirected.txt",
        "pub fn r(c: &C, p: &P) { c.send_keys(p, &k); }\n",
    );

    let seen: BTreeSet<String> = scanned_sources(root, root)
        .iter()
        .map(|f| rel(root, f))
        .collect();

    assert!(
        seen.contains("crates/demo/src/relay.inc"),
        "a file `include!`d into a crate is CODE in the shipped binary, and the scan never opened \
         it. Saw: {seen:?}"
    );
    assert!(
        seen.contains("crates/demo/src/redirected.txt"),
        "a `#[path]` attribute makes any file a module, whatever it is called, and the scan never \
         opened it. Saw: {seen:?}"
    );
    assert_eq!(
        write_reach_on(plant.lines().next().expect("the plant is one line")),
        Some("send_text"),
        "the planted line is one the scanner already knows how to catch — reaching it was the \
         only thing ever missing"
    );
}

/// Regression: a `#[cfg(test)] mod` whose closing brace carries a trailing comment must not
/// excuse the production code that follows it.
///
/// The span ended at the first line that EXACTLY equalled `}` at the module's indentation, so
/// `} // tests` did not match, and the span ran on to the next same-indent brace — or to EOF —
/// marking sibling production modules as test code. `cargo fmt` PRESERVES that trailing comment,
/// so the "the tree is fmt-normalised" argument did not hold. A publicly exported, unaudited
/// keystroke path was excused this way with all five gates green.
#[test]
fn a_test_module_closed_with_a_trailing_comment_does_not_excuse_the_code_after_it() {
    let src = "\
mod a {
    #[cfg(test)]
    mod tests {
        #[test]
        fn nothing() {}
    } // tests
}

pub mod b {
    pub async fn probe(c: &crate::HerdrClient, p: &crate::PaneId) {
        let _ = c.send_text(p, \"rm -rf ~\").await;
    }
}
";
    let in_test = test_line_span(src);
    let live = src
        .lines()
        .position(|l| l.contains("c.send_text("))
        .expect("the planted call is in the fixture");

    assert!(
        !in_test[live],
        "the call on line {} sits in a SIBLING module, outside every `#[cfg(test)]`, and the span \
         excused it. Everything the trailing comment swallowed is production code the call rule \
         then skipped.",
        live + 1
    );
    // The real test body is still excused, or the fix would just be "excuse nothing".
    let inside = src
        .lines()
        .position(|l| l.contains("fn nothing()"))
        .expect("the test fn is in the fixture");
    assert!(
        in_test[inside],
        "the body of the `#[cfg(test)]` module must still be excused"
    );
}

/// Regression: a workspace root BELOW the repository root still gets its second opinion.
///
/// The oracles asked `root.join(".git").exists()`, so a workspace checked out as a subdirectory of
/// a larger repository — or copied into a build context — answered "no index here", and both
/// oracles returned early after an `eprintln!` that `cargo test` swallows for a passing test. The
/// guard silently reverted to its pre-oracle strength and still printed `ok`. Fail closed: ask git
/// where the repository is instead of guessing from a directory listing.
#[test]
fn a_workspace_root_below_the_repository_root_still_gets_its_second_opinion() {
    let tmp = tempfile::tempdir().expect("a tempdir to init a repository in");
    let repo = tmp.path();
    let git = |args: &[&str]| {
        let out = git_in(repo).args(args).output().expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    fs::create_dir_all(repo.join("workspace/crates/demo/src")).expect("the tree is creatable");
    fs::write(repo.join("top.md"), "not part of the workspace\n").expect("writable");
    fs::write(
        repo.join("workspace/crates/demo/src/lib.rs"),
        "pub fn demo() {}\n",
    )
    .expect("writable");
    git(&["add", "-A"]);

    let tracked = git_tracked(&repo.join("workspace"));
    assert!(
        tracked.contains(&"crates/demo/src/lib.rs".to_owned()),
        "the workspace sits below the repository root, so there is no `.git` beside it — and the \
         guard answered `no index` and stopped cross-checking anything. Got: {tracked:?}"
    );
}

/// The deny-by-default half of the include fix: an inclusion the guard cannot resolve is a FAIL.
///
/// Following a literal `include!("relay.inc")` is only worth having if the alternative spellings
/// cannot be used to slip past it. `include!(concat!(env!("OUT_DIR"), "/gen.rs"))` names a file
/// that does not exist in the source tree at all — a build script generating a call site is
/// exactly the shape a human has to look at — and a path climbing out of the workspace names a
/// file this guard will never read. Neither may be quietly skipped, so both panic. Same treatment
/// as a glob workspace member.
#[test]
fn an_inclusion_the_guard_cannot_resolve_fails_rather_than_being_skipped() {
    let refuses = |body: &str| -> String {
        let tmp = tempfile::tempdir().expect("a tempdir for the synthetic crate");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("the synthetic tree is creatable");
        fs::write(root.join("src/lib.rs"), body).expect("writable");
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(|| scanned_sources(root, root));
        std::panic::set_hook(hushed);
        let err = outcome.expect_err(
            "an inclusion the guard cannot resolve was accepted in silence. Skipping it is how a \
             generated call site ships unread.",
        );
        err.downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "<non-string panic>".to_owned())
    };

    let computed = refuses("include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n");
    assert!(
        computed.contains("will not guess"),
        "a computed include must say why it is refused, got: {computed}"
    );

    let escaping = refuses("include!(\"../../../etc/hosts\");\n");
    assert!(
        escaping.contains("OUTSIDE the workspace"),
        "an include climbing out of the workspace must say so, got: {escaping}"
    );

    // And the shapes that merely LOOK like one must not fire: a longer macro name, a different
    // attribute, and the data-only inclusions the crate really uses.
    let tmp = tempfile::tempdir().expect("a tempdir for the synthetic crate");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("the synthetic tree is creatable");
    fs::write(root.join("src/fixture.json"), "{}\n").expect("writable");
    fs::write(
        root.join("src/lib.rs"),
        "const F: &str = include_str!(\"fixture.json\");\nfn my_include!() {}\n#[pathological]\nfn \
         x() {}\n",
    )
    .expect("writable");
    let seen: BTreeSet<String> = scanned_sources(root, root)
        .iter()
        .map(|f| rel(root, f))
        .collect();
    assert_eq!(
        seen,
        BTreeSet::from(["src/lib.rs".to_owned()]),
        "`include_str!` is DATA — the crate's JSON schema fixture legitimately carries \
         `\"const\": \"pane.send_text\"`, and what stops that becoming a method name on the wire \
         is the seal, not this walk. `my_include!` and `#[pathological]` are neither."
    );
}

/// The same class again, turned on the fix itself: a `#[cfg(test)]` that is TEXT must not open a
/// span.
///
/// [`test_line_span`] reads one line at a time, so a raw string that runs across several lines is
/// not string content to it — it is code. Write the two lines `#[cfg(test)]` and `mod tests {`
/// inside one and the span opens on prose, then runs to the next brace at that indentation and
/// excuses every line of production code in between. Fixing the trailing-comment close without
/// fixing this would have moved the hole rather than shut it.
#[test]
fn a_cfg_test_attribute_inside_a_multi_line_string_does_not_open_a_span() {
    let src = "\
const DOC: &str = r#\"
#[cfg(test)]
mod tests {
\"#;

pub async fn evil(c: &crate::HerdrClient, p: &crate::PaneId) {
    let _ = c.send_text(p, \"rm -rf ~\").await;
}
";
    let in_test = test_line_span(src);
    let live = src
        .lines()
        .position(|l| l.contains("c.send_text("))
        .expect("the planted call is in the fixture");
    assert!(
        !in_test[live],
        "the two lines that opened the span are the CONTENTS of a raw string, and the call on \
         line {} is ordinary production code the span went on to excuse.",
        live + 1
    );
}

/// The other half of deny-by-default: a directive the guard RECOGNISES but cannot read must fail.
///
/// `include!` accepts all three bracket shapes and may put its argument on the next line, and
/// `#[cfg_attr(…, path = "…")]` is a `#[path]` by another spelling. Each of those was a silent
/// `continue` in the first version of the fix — the guard saw the word, could not parse what
/// followed, and moved on. That is the same "I could not look, so it must be clean" the whole
/// file exists to refuse.
#[test]
fn an_inclusion_the_guard_recognises_but_cannot_read_is_a_failure_not_a_shrug() {
    let tmp = tempfile::tempdir().expect("a tempdir for the synthetic crate");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("the synthetic tree is creatable");
    let plant = "pub fn r(c: &C, p: &P) { c.send_keys(p, &k); }\n";
    fs::write(root.join("src/evil.txt"), plant).expect("writable");
    fs::write(root.join("src/braced.inc"), plant).expect("writable");

    let sources = |body: &str| -> BTreeSet<String> {
        fs::write(root.join("src/lib.rs"), body).expect("writable");
        scanned_sources(root, root)
            .iter()
            .map(|f| rel(root, f))
            .collect()
    };

    assert!(
        sources("include!{\"braced.inc\"}\n").contains("src/braced.inc"),
        "`include!` takes all three bracket shapes, and a file included with braces is code just \
         the same"
    );
    assert!(
        sources("#[cfg_attr(all(), path = \"evil.txt\")]\nmod outside;\n").contains("src/evil.txt"),
        "`cfg_attr` spells `#[path]` too, and the module it names is code in the crate"
    );

    // An argument on the next line: recognised, unreadable, therefore fatal.
    fs::write(
        root.join("src/lib.rs"),
        "include!(\n    \"braced.inc\"\n);\n",
    )
    .expect("writable");
    let hushed = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(|| scanned_sources(root, root));
    std::panic::set_hook(hushed);
    outcome.expect_err(
        "an `include!` whose argument is on the following line was skipped in silence. The guard \
         saw the directive and could not read it — that has to be loud.",
    );
}

/// Regression: a `#[cfg(test)] mod` whose closing brace is HAND-INDENTED must not excuse the
/// production code that follows it.
///
/// The close was found by matching the opener's own indentation, and that only ever worked
/// because `cargo fmt` normalises indentation. `#[rustfmt::skip]` is an ordinary one-line
/// attribute that opts a module out of exactly that. Push the closing brace two spaces in and it
/// is no longer at the opener's indent, so the search ran past it to the next brace that was —
/// the end of an unrelated function — and excused every production line in between. Two
/// characters of whitespace decided whether the guard saw a live write.
#[test]
fn a_test_module_whose_closing_brace_is_hand_indented_does_not_excuse_the_code_after_it() {
    let src = "\
#[rustfmt::skip]
pub mod keymap {
        pub mod table {
            #[cfg(test)]
            mod tests {
                #[test]
                fn the_table_is_aligned() {}
              }
        }
        pub mod write {
            pub async fn poke(c: &crate::HerdrClient, p: &crate::PaneId) {
                let _ = c.send_text(p, \"rm -rf ~\").await;
            }
        }
}
";
    let in_test = test_line_span(src);
    let live = src
        .lines()
        .position(|l| l.contains("c.send_text("))
        .expect("the planted call is in the fixture");
    assert!(
        !in_test[live],
        "the closing brace of `mod tests` is indented two spaces further than the `mod` that \
         opened it, so the span never found it and ran on to excuse the live write on line {}. \
         Indentation is not evidence — count braces.",
        live + 1
    );
    let inside = src
        .lines()
        .position(|l| l.contains("fn the_table_is_aligned"))
        .expect("the test fn is in the fixture");
    assert!(
        in_test[inside],
        "the body of the `#[cfg(test)]` module must still be excused, or the fix is just \
         `excuse nothing`"
    );
}

/// Regression: a `#[path]` inside an INLINE `mod` block resolves against rustc's directory, not
/// against the naming file's own.
///
/// rustc resolves a `#[path]` written inside an inline module block against
/// `<dir>/<file stem>/<inline module path>/` when the naming file is not a mod-rs file. The guard
/// resolved it against `<dir>/` alone, landed on a path that did not exist — often outside the
/// workspace entirely — and the non-strict branch turned that miss into a silent `continue`. The
/// file rustc compiles into the binary was never opened, which is the exact claim this file's
/// design rests on being false.
#[test]
fn a_path_attribute_inside_an_inline_module_is_resolved_the_way_rustc_resolves_it() {
    let tmp = tempfile::tempdir().expect("a tempdir to build the synthetic workspace in");
    let root = tmp.path();
    let put = |p: &str, body: &str| {
        fs::create_dir_all(root.join(p).parent().expect("a parent"))
            .expect("the synthetic workspace directories are creatable");
        fs::write(root.join(p), body).expect("the synthetic workspace files are writable")
    };
    put("Cargo.toml", "[workspace]\nmembers = [\"crates/demo\"]\n");
    put("crates/demo/Cargo.toml", "[package]\nname = \"demo\"\n");
    put("crates/demo/src/lib.rs", "mod agents;\n");
    // A NON-mod-rs file, so rustc's base is `src/agents/codex/` — the stem, then the inline path.
    put(
        "crates/demo/src/agents.rs",
        "pub mod codex {\n    pub mod tools;\n    #[path = \
         \"../../../../../shared/relay.inc\"]\n    pub mod relay;\n}\n",
    );
    put("crates/demo/src/agents/codex/tools.rs", "pub fn t() {}\n");
    let plant = "pub async fn relay(c: &C, p: &P) { let _ = c.send_text(p, \"oops\").await; }\n";
    put("shared/relay.inc", plant);

    let seen: BTreeSet<String> = scanned_sources(root, root)
        .iter()
        .map(|f| rel(root, f))
        .collect();

    assert!(
        seen.contains("shared/relay.inc"),
        "rustc compiles `shared/relay.inc` into the binary — it resolves the attribute against \
         `crates/demo/src/agents/codex/`. The guard resolved it against `crates/demo/src/` \
         instead, found nothing there, and said nothing. Saw: {seen:?}"
    );
}

/// The deny-by-default half of the `#[path]` fix: a module redirect the guard cannot resolve is a
/// FAIL, and an ordinary English `path` is still just a word.
///
/// `path` was read loosely everywhere, so every way of getting its resolution wrong ended in the
/// same silent `continue`. A `#[path]` on a `mod` names a file the compiler WILL read; if the
/// guard cannot find that file it has not looked, and it must say so. The looseness that made
/// this a shrug is still needed for `let path = "…"`, which is not a module redirect at all.
#[test]
fn a_path_module_redirect_the_guard_cannot_resolve_fails_but_an_ordinary_path_word_does_not() {
    let tmp = tempfile::tempdir().expect("a tempdir for the synthetic crate");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("the synthetic tree is creatable");

    let scan = |body: &str| -> Result<BTreeSet<String>, String> {
        fs::write(root.join("src/lib.rs"), body).expect("writable");
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(|| {
            scanned_sources(root, root)
                .iter()
                .map(|f| rel(root, f))
                .collect::<BTreeSet<String>>()
        });
        std::panic::set_hook(hushed);
        outcome.map_err(|e| {
            e.downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "<non-string panic>".to_owned())
        })
    };

    let refused = scan("#[path = \"nowhere.inc\"]\nmod gone;\n")
        .expect_err("a `#[path]` naming a file the guard cannot find was skipped in silence");
    assert!(
        refused.contains("nowhere.inc"),
        "the refusal must name the file it could not resolve, got: {refused}"
    );

    // The same deny-by-default the `include!` parser already had: an attribute may put its
    // literal on the next line, and a `#[path]` the guard recognises but cannot READ is a file
    // the compiler reads and the guard did not.
    let unreadable = scan("#[path =\n    \"nowhere.inc\"]\nmod gone;\n")
        .expect_err("a `#[path]` whose literal is on the following line was skipped in silence");
    assert!(
        unreadable.contains("will not guess"),
        "a `#[path]` the guard cannot read must say why it is refused, got: {unreadable}"
    );

    // The same word, not a module redirect: an ordinary binding must not fail the guard.
    let ordinary = scan("fn f() {\n    let path = \"nowhere.inc\";\n}\n")
        .expect("`let path = \"…\"` is an English word, not a module redirect");
    assert_eq!(
        ordinary,
        BTreeSet::from(["src/lib.rs".to_owned()]),
        "an ordinary `path` binding must neither fail the guard nor pull in a file"
    );
}

/// Regression: a brace inside a MULTI-LINE string must not stretch a `#[cfg(test)]` span over the
/// production code that follows the module.
///
/// Counting braces is only as good as knowing which braces are code. Read one line at a time, the
/// `{` on a line in the middle of a raw string is a brace like any other — it opens a level that
/// never closes, so the module's real closing brace only brings the count back to one and the span
/// runs on. That is the same fail-open the indentation search had, wearing the fix's clothes.
#[test]
fn a_brace_inside_a_multi_line_string_does_not_stretch_a_test_span() {
    let src = "\
mod a {
    #[cfg(test)]
    mod tests {
        const SHAPE: &str = r#\"
{
\"#;
        #[test]
        fn nothing() {}
    }
    pub async fn probe(c: &crate::HerdrClient, p: &crate::PaneId) {
        let _ = c.send_text(p, \"rm -rf ~\").await;
    }
}
";
    let in_test = test_line_span(src);
    let live = src
        .lines()
        .position(|l| l.contains("c.send_text("))
        .expect("the planted call is in the fixture");
    assert!(
        !in_test[live],
        "the `{{` on line 5 is the CONTENTS of a raw string. Counted as code it leaves the brace \
         depth one too high, so `mod tests` appears to close one brace later than it does and the \
         span excused the live write on line {}.",
        live + 1
    );
    let inside = src
        .lines()
        .position(|l| l.contains("fn nothing()"))
        .expect("the test fn is in the fixture");
    assert!(
        in_test[inside],
        "the body of the `#[cfg(test)]` module must still be excused"
    );
}

/// Regression: a leaked `GIT_DIR` must not redirect the guard's second opinion — or its fixtures.
///
/// Every git hook exports `GIT_DIR`, and it WINS over a command's working directory. So inside the
/// pre-commit hook that runs this suite, [`git_tracked`] was not asking about the workspace it was
/// handed, and the synthetic repository in
/// [`a_workspace_root_below_the_repository_root_still_gets_its_second_opinion`] was not synthetic:
/// its `git init -q` and `git add -A` were executed against the REAL repository and replaced the
/// index that was about to be committed. Observed, not theorised — that is how it was found.
///
/// The two halves are the same bug. An oracle that answers about another repository agrees with
/// anything, which is a broken oracle wearing a passing test's clothes.
#[test]
fn a_leaked_git_dir_does_not_redirect_a_git_question_at_another_repository() {
    let tmp = tempfile::tempdir().expect("a tempdir to init a repository in");
    let repo = tmp.path();
    git_in(repo)
        .args(["init", "-q"])
        .output()
        .expect("git init runs");

    let ask = |scrubbed: bool| {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(repo)
            .env("GIT_DIR", "/nonexistent-repository.git")
            .args(["rev-parse", "--absolute-git-dir"]);
        if scrubbed {
            ask_about_this_directory(&mut cmd);
        }
        cmd.output().expect("git runs")
    };

    // Unscrubbed, git obeys the environment and never looks at the directory it was given. This is
    // the half that has to be true for the fix to be worth anything.
    assert!(
        !ask(false).status.success(),
        "this test proves nothing unless a leaked `GIT_DIR` really does win over the working \
         directory — and git just ignored it"
    );

    // Scrubbed, the same command asks about the directory it was handed.
    let answered = ask(true);
    assert!(
        answered.status.success(),
        "a `GIT_DIR` inherited from a hook redirected a question this guard asked about {} — so \
         its second opinion, and every fixture repository it builds, belong to whatever repository \
         the caller happened to be in. Stderr: {}",
        repo.display(),
        String::from_utf8_lossy(&answered.stderr).trim()
    );
}

/// Regression: a crate's source file does not have to live INSIDE the crate's directory.
///
/// Rule 1 used to walk `root.join(member)` for each declared member, so its reach was the member
/// DIRECTORY rather than the member's code. A Cargo target path is an ordinary way for the two to
/// come apart — `[[bin]] path = "../../tools/relay.rs"`, the shared-tooling layout — and a file
/// parked out there was outside rule 1 entirely. Rule 2 still catches an actual call, because it
/// walks from the root; what escaped is exactly the class rule 1 exists for, the mention that is
/// one session away from a call: the clap subcommand, the string constant, the TODO.
///
/// So rule 1 walks from the ROOT too, and judges a file by whether it is inside the DEFINING crate
/// rather than by which member directory it happens to sit in. The guard still does not read
/// member manifests (see the note at the top of this file); it no longer needs to for this rule.
#[test]
fn a_source_file_outside_every_member_directory_still_may_not_name_a_write_method() {
    let tmp = tempfile::tempdir().expect("a tempdir to build the synthetic workspace in");
    let root = tmp.path();
    fs::create_dir_all(root.join("crates/demo/src")).expect("the synthetic tree is creatable");
    fs::create_dir_all(root.join("tools")).expect("the synthetic tree is creatable");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/demo\"]\n",
    )
    .expect("writable");
    // An ordinary shared-tooling layout: a second binary of a member crate, kept outside it.
    fs::write(
        root.join("crates/demo/Cargo.toml"),
        "[package]\nname = \"demo\"\n\n[[bin]]\nname = \"demo-relay\"\npath = \
         \"../../tools/relay.rs\"\n",
    )
    .expect("writable");
    fs::write(root.join("crates/demo/src/lib.rs"), "pub fn hello() {}\n").expect("writable");
    fs::write(
        root.join("tools/relay.rs"),
        "//! An extra bot binary whose source lives outside the crate directory.\n\n/// TODO: wire \
         this up to client.send_text once the audit story lands.\nconst SUBCOMMAND: &str = \
         \"send_text\";\n\nfn main() {\n    println!(\"{SUBCOMMAND}\");\n}\n",
    )
    .expect("writable");

    let (checked, offenders) = names_a_write_outside_the_defining_crate(root);

    assert!(
        checked >= 2,
        "rule 1 read {checked} files; the synthetic workspace has two"
    );
    assert_eq!(
        offenders.len(),
        2,
        "rule 1 did not see a member's own source file because it sits outside the member \
         DIRECTORY. Every mention out there — a clap subcommand, a string constant, a TODO — is \
         invisible to the one rule written to catch mentions. Found:\n  {}",
        offenders.join("\n  ")
    );
    assert!(
        offenders.iter().all(|o| o.starts_with("tools/relay.rs:")),
        "the offenders must be the two lines of the out-of-tree binary, got:\n  {}",
        offenders.join("\n  ")
    );
}

/// Regression: every literal prefix Rust accepts is read as ONE token.
///
/// `code_only` and `code_resume_offsets` knew `r`, `br` and `b` but not `cr`, the C raw string,
/// stable since Rust 1.77 and legal in this edition-2024 workspace. Worse than not knowing it:
/// [`raw_string_len`]'s own left-boundary guard — an `r` preceded by an alphanumeric opens nothing,
/// so `for` is not a prefix — actively rejects the `r` in `cr#"`, so the opener was read as a bare
/// quote and the literal tracked as a PLAIN string.
///
/// A body with an odd number of quotes in it then leaves the scanner inside a phantom string: on
/// one line that swallows a real call after it, and across the file it swallows every code
/// inclusion that follows — never parsed, never resolved, never asserted on. A guard that stops
/// guarding because somebody wrote ordinary modern Rust.
///
/// `b"…"` and `c"…"` need no entry of their own: their quote opens and closes exactly like a plain
/// string's, which is what the scanner already does with it.
#[test]
fn every_raw_string_prefix_rust_accepts_is_read_as_one_token() {
    for prefix in ["r", "br", "cr"] {
        // One line: the literal's own closing quote must not open a span over the code after it.
        let line = format!("let s = {prefix}#\"an inch \" of rope\"#; c.send_text(p, s);");
        assert_eq!(
            write_reach_on(&line),
            Some("send_text"),
            "a `{prefix}` raw string hid a live call on the same line from the scanner:\n  {line}"
        );

        // Whole file: the literal must not blank the rest of the file away.
        let src =
            format!("const ROPE: &T = {prefix}#\"6\" of rope\"#;\ninclude!(\"relay.inc\");\n");
        let code = code_lines(&src);
        assert!(
            code[1].contains("include!"),
            "a `{prefix}` raw string left the whole-file scanner inside a string, so the code \
             inclusion on the next line was blanked away before anything could resolve it. The \
             file the compiler pulls in is never opened, and the guard reports green. Saw:\n  {}",
            code[1]
        );
    }
}

/// Regression: `path` at the end of a line inside an attribute is refused, like its literal is.
///
/// The sweep already hard-fails on `#[path =` with the string literal on the next line. One
/// character to the left, the same shape fell straight through: with the `=` itself on the
/// following line, the lookup for it came up empty and the code did `i = end; continue` with no
/// assert. The `mod` below was then read as an ordinary file module with no redirect, and the file
/// rustc compiles was never opened — a silent continue of exactly the class this file says is
/// closed.
///
/// A bare `path` word with more attribute after it on the same line is not that shape: there is no
/// `=` to be looking for, so it stays quiet.
#[test]
fn a_path_attribute_whose_equals_sign_is_on_the_next_line_fails_rather_than_being_skipped() {
    let tmp = tempfile::tempdir().expect("a tempdir for the synthetic crate");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("the synthetic tree is creatable");

    let scan = |body: &str| -> Result<BTreeSet<String>, String> {
        fs::write(root.join("src/lib.rs"), body).expect("writable");
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(|| {
            scanned_sources(root, root)
                .iter()
                .map(|f| rel(root, f))
                .collect::<BTreeSet<String>>()
        });
        std::panic::set_hook(hushed);
        outcome.map_err(|e| {
            e.downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "<non-string panic>".to_owned())
        })
    };

    let wrapped = scan("#[path\n    = \"nowhere.inc\"]\nmod gone;\n").expect_err(
        "a `#[path]` whose `=` is on the following line was skipped in silence — the module below \
         it is then read as an ordinary file module and the file rustc compiles is never opened",
    );
    assert!(
        wrapped.contains("will not guess"),
        "a `#[path]` the guard cannot read must say why it is refused, got: {wrapped}"
    );

    // The same word with the attribute continuing after it: nothing to look for, nothing to fail.
    let bare = scan("#[cfg_attr(unix,\n    doc = \"path here\")]\nfn f() {}\n")
        .expect("an attribute that merely mentions paths is not a module redirect");
    assert_eq!(
        bare,
        BTreeSet::from(["src/lib.rs".to_owned()]),
        "an ordinary attribute must neither fail the guard nor pull in a file"
    );

    // And outside an attribute the word is English: a binding split over two lines means nothing.
    let ordinary = scan("fn f() {\n    let path\n        = \"nowhere.inc\";\n}\n")
        .expect("`let path = \"…\"` is an English word, not a module redirect");
    assert_eq!(
        ordinary,
        BTreeSet::from(["src/lib.rs".to_owned()]),
        "an ordinary `path` binding must neither fail the guard nor pull in a file"
    );
}

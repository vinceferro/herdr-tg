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
//! So the walk now starts at the WORKSPACE ROOT and scans every `.rs` file it finds, skipping only
//! `target/` and `.git/`. A crate added tomorrow is covered the day it is added rather than the day
//! someone remembers to add it here. Three properties make that real rather than nominal:
//!
//! 1. **An unreadable directory or file FAILS.** The old walk had `Err(_) => continue`, so a
//!    permission error read as "nothing to see". A guard that cannot see must not report green.
//! 2. **Every declared workspace member must contribute at least one scanned file.** A renamed or
//!    moved crate makes the scan vacuous otherwise — green because it looked nowhere.
//! 3. **The exemptions are a short explicit list of FILES, asserted to exist.** Not a directory
//!    prefix, not a pattern. If `wire.rs` is renamed its exemption goes stale, the new file gets
//!    scanned, and the suite goes red — the safe direction.
//!
//! # The three rules
//!
//! 1. **No workspace member except `herdr-client` may so much as NAME the three methods.** Not a
//!    call, not an import, not a clap subcommand, not a `//` TODO that a later session turns into
//!    one. Those crates are what a timer, a cron job or (from slice 2) a Telegram message can
//!    reach; the client crate, which defines them, is not.
//! 2. **A *call* may appear nowhere in the workspace outside `#[cfg(test)]`** — including via UFCS
//!    (`HerdrClient::send_text(c, p, …)`), which the old `.send_text(`-only spelling missed.
//! 3. **`Request` must stay SEALED.** The seal is what makes rules 1 and 2 more than a grep: an
//!    unsealed public `Request` lets any crate hand `HerdrClient::call` a `const METHOD` that never
//!    spells `send_text` at all (`concat!("pane.send", "_text")`), so no textual rule can see it.
//!    Rule 3 is a tripwire on the declaration; the compiler is the actual enforcement.
//!
//! It reads only files inside this repository. It opens no socket and touches no herd.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The three methods, spelled as they appear anywhere at all.
const WRITE_NAMES: [&str; 3] = ["send_text", "send_keys", "send_input"];

/// Directory names never walked. `target/` holds build-script-generated `.rs` and vendored code;
/// `.git/` holds packfiles. Nothing a human writes lives in either.
const SKIP_DIRS: [&str; 2] = ["target", ".git"];

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

/// The workspace root: `crates/herdr-client/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-client is two levels below the workspace root")
        .to_path_buf()
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
                if SKIP_DIRS.iter().any(|s| *s == name) {
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
    let start = manifest
        .find("members")
        .expect("the workspace manifest declares `members`");
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

/// The 0-based line numbers that sit inside a `#[cfg(test)]` module.
///
/// The whole workspace is `cargo fmt`-normalised, so a test module is always the exact line
/// `#[cfg(test)]` followed by a `mod …{` at the same indentation, closed by a `}` at that same
/// indentation. That is a narrow rule on purpose: it CANNOT accidentally mark production code as
/// test code, because a mismatch just means fewer lines are excused and the test gets stricter.
fn test_line_span(src: &str) -> Vec<bool> {
    let lines: Vec<&str> = src.lines().collect();
    let mut in_test = vec![false; lines.len()];

    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            let indent = lines[i].len() - lines[i].trim_start().len();
            // The item it applies to. Only a `mod` opens a span worth tracking; a `#[cfg(test)]`
            // on a single `use` or `fn` covers no call sites we would otherwise excuse.
            if let Some(open) = lines.get(i + 1)
                && open.trim_start().starts_with("mod ")
                && open.trim_end().ends_with('{')
            {
                let close = format!("{}}}", " ".repeat(indent));
                let end = lines
                    .iter()
                    .enumerate()
                    .skip(i + 2)
                    .find(|(_, l)| **l == close)
                    .map_or(lines.len() - 1, |(n, _)| n);
                for flag in in_test.iter_mut().take(end + 1).skip(i) {
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

/// The line with every string-literal span and every trailing `//` comment blanked to spaces,
/// so a scan sees CODE and only code. Byte indices are preserved, so a hit still maps back to the
/// original text for the failure message.
///
/// `const METHOD: &'static str = "pane.send_text";` and
/// `tracing::warn!(… "pane.send_keys called with NO keys" …)` are the real lines this has to let
/// through: they name the method in data, and blanking string spans is what tells them apart from
/// a reference to the function. Raw strings blank correctly too (`r#"…"#` opens on its `"`).
///
/// CHAR LITERALS ARE CONSUMED BEFORE STRINGS, and that ordering is load-bearing. A `'"'`
/// char literal used to be read as a string OPENER, which blanked everything after it —
/// so a real `c.send_text(..)` later on the same line became invisible to the guard. That
/// is a FAIL-OPEN, not the fail-closed the old comment claimed: blanking more code means
/// the scanner sees less, never more. A reviewer drove `pane.send_text` onto the wire
/// through the shipped binary with this test green. Do not remove the char-literal arm.
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

/// Rule 1 — no workspace member except the defining crate may NAME a write method, anywhere.
///
/// Scoped to whole members rather than to `crates/herdr-tg/src`, so a crate added in slice 2 is
/// covered on the commit that adds it. Comments and string literals count: a `//` TODO naming the
/// method is one session away from being a call.
#[test]
fn no_member_outside_the_client_crate_may_even_name_a_write_method() {
    let root = workspace_root();
    let members = workspace_members(&root);
    let mut offenders = Vec::new();
    let mut checked = 0usize;

    for member in members.iter().filter(|m| m.as_str() != DEFINING_MEMBER) {
        let dir = root.join(member);
        assert!(
            dir.is_dir(),
            "workspace member `{member}` is declared in Cargo.toml but {} does not exist — the \
             guard cannot scan it and will not pretend it did",
            dir.display()
        );
        let files = rust_files(&dir);
        assert!(
            !files.is_empty(),
            "workspace member `{member}` yielded ZERO .rs files. A vacuous scan is not a pass."
        );
        for file in files {
            checked += 1;
            let src = fs::read_to_string(&file).expect("a source file in this repo is readable");
            for (n, line) in src.lines().enumerate() {
                for name in WRITE_NAMES {
                    if line.contains(name) {
                        offenders.push(format!("{}:{}: {}", rel(&root, &file), n + 1, line.trim()));
                    }
                }
            }
        }
    }

    assert!(
        checked > 0,
        "scanned zero files across every non-client workspace member — the guard is vacuous"
    );
    assert!(
        offenders.is_empty(),
        "a workspace member outside `{DEFINING_MEMBER}` names a write method. These crates are \
         what a timer, a cron job or a Telegram message can reach; slice 1 ships the writes with \
         NO path from any of them (spec delta #24). Found:\n  {}",
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
    let all = rust_files(&root);
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
        if CALL_RULE_EXEMPT.contains(&relp.as_str()) {
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

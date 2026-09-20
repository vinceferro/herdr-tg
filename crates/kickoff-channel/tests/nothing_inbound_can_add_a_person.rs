//! Nothing that arrives from Telegram — a typed line, a tap, a command — can let a person speak.
//!
//! The people who may speak are a list, and the only way onto it is argv at a keyboard: the bot's
//! own list comes from the configuration, and a project's from `kickoff-channel allow <repo> <user>`.
//! "Not implemented" is not the property; the property is that the code which reads inbound
//! content has no road to the code which writes the list.
//!
//! This is a SCANNER over the shipped source, not the compiler: it pins the roads that exist and
//! the shapes a road would have to take, and it fails the build when one appears. What it pins:
//! the registry's one setter is named only where argv reaches it; the terminal verbs (`cmd::`)
//! are named only by the file that dispatches argv; the registry hands out no way to change a
//! project other than through its own methods, in any visibility, and grows no `impl` block
//! elsewhere; and the bot's command set is a closed list of two, whatever shape a third would
//! take. What it does not pin: a second setter written into `registry.rs` on purpose, or a direct
//! write of the registry file. Those are a door somebody built, not one somebody walked through.
//!
//! The window it reads through is checked too. It used to stop at the first `#[cfg(test)]` in a
//! file, and `hub.rs` carries test-only builders ABOVE its inbound paths — so `relay()`,
//! `resolve_tap()` and `standing_of()` were never read, and the setter named inside `relay()`
//! passed. Now the cut is the test MODULE, and the guard refuses to run if the inbound paths are
//! not inside what it is looking at.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every `.rs` under `src/`, with the path each is reported by.
fn sources() -> Vec<(String, String)> {
    let root = crate_root();
    let mut out = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("src is readable").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                let rel = p.strip_prefix(&root).unwrap_or(&p).display().to_string();
                out.push((rel, std::fs::read_to_string(&p).expect("readable")));
            }
        }
    }
    out.sort();
    out
}

/// The part of a file that ships: everything before its `#[cfg(test)] mod tests`, in either the
/// inline or the `mod tests;` form. A test may name the setter — that is how the gate is tested —
/// and nothing a test names is reachable by a message.
///
/// The cut is the test MODULE and not the first `#[cfg(test)]` attribute, because a test-only
/// helper sitting above the code that matters would otherwise hide everything below it from the
/// guard. A file with no test module is read whole, and so is a `#[cfg(test)]` helper — a
/// helper that names the setter fails the build too, which is the safe side to fail on.
fn shipped(src: &str) -> &str {
    match src.find("\n#[cfg(test)]\nmod tests") {
        Some(at) => &src[..at],
        None => src,
    }
}

/// Every function signature in `src`: from `fn ` to the brace or semicolon that ends it, so a
/// return type on its own line is still part of the signature it belongs to.
fn signatures(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find("fn ") {
        let from = &rest[at..];
        let end = from.find(['{', ';']).unwrap_or(from.len());
        out.push(from[..end].split_whitespace().collect::<Vec<_>>().join(" "));
        rest = &from[end..];
    }
    out
}

/// The files that dispatch argv, and so may name the terminal verbs.
///
/// `src/lib.rs` is what `src/main.rs` became when the crate was renamed and grew two binaries: the
/// clap dispatch moved there whole, and the shims under `src/bin/` are three lines that call into
/// it. Those shims are deliberately NOT listed — they name no verb and reach into no `cmd::`, so
/// exempting them would widen this door without anything asking to come through it.
fn dispatches_argv(path: &str) -> bool {
    path == "src/lib.rs" || path.starts_with("src/cmd/")
}

#[test]
fn no_message_tap_or_command_can_add_a_person_to_any_list() {
    let sources = sources();
    let read = |name: &str| -> &str {
        let (_, src) = sources
            .iter()
            .find(|(p, _)| p == name)
            .unwrap_or_else(|| panic!("{name} is where it was"));
        shipped(src)
    };

    // 0. The guard is looking at the inbound paths. If any of these names is missing from the
    //    shipped slice, the window has shrunk — a cut landed above the code that reads a message
    //    or a tap — and every check below would pass for the wrong reason.
    for (file, must_hold) in [
        (
            "src/hub.rs",
            &[
                "pub async fn relay(",
                "pub async fn resolve_tap(",
                "pub async fn standing_of(",
                "async fn serve_connection(",
            ][..],
        ),
        (
            "src/bot.rs",
            &[
                "async fn on_message(",
                "async fn on_callback(",
                "pub enum Command {",
            ][..],
        ),
        (
            "src/registry.rs",
            &["pub fn set_may_speak(", "pub fn enrol("][..],
        ),
    ] {
        let src = read(file);
        for name in must_hold {
            assert!(
                src.contains(name),
                "the guard is no longer reading the inbound paths: `{name}` is not in the \
                 shipped part of {file}. Either it moved, or a `#[cfg(test)] mod tests` now sits \
                 above it and the slice this guard scans has shrunk."
            );
        }
    }

    // 1. The setter is named only where argv reaches it. `bot.rs` reads Telegram; `hub.rs` reads
    //    the socket and resolves taps; neither may so much as spell the name. A grep, because the
    //    failure this guards against is not a compile error: somebody adds a convenient `/allow`
    //    handler, and it compiles.
    let may_name_the_setter = ["src/registry.rs", "src/cmd/enroll.rs"];
    let mut named_it = Vec::new();
    for (path, src) in &sources {
        if path.ends_with("/tests.rs") || may_name_the_setter.contains(&path.as_str()) {
            continue;
        }
        if shipped(src).contains("set_may_speak") {
            named_it.push(path.clone());
        }
    }
    assert!(
        named_it.is_empty(),
        "the setter that lets a person speak is named outside the terminal-only door: \
         {named_it:?}. Only `kickoff-channel allow <repo> <user>` at a keyboard may add a person; a \
         message, a tap or a command must not be able to reach it."
    );

    // 1b. And the terminal verbs are reached only from argv. `cmd::enroll::let_speak` is the
    //     `pub(crate)` wrapper the verb calls, and a wrapper is a second name for the same door —
    //     so the file that reads Telegram must not name `cmd::` at all, whatever the wrapper is
    //     called this week. `lib.rs` dispatches argv and is the one file outside `cmd/` allowed.
    let mut reached_into_cmd = Vec::new();
    for (path, src) in &sources {
        if path.ends_with("/tests.rs") || dispatches_argv(path) {
            continue;
        }
        let src = shipped(src);
        if src.contains("cmd::") || src.contains("let_speak") {
            reached_into_cmd.push(path.clone());
        }
    }
    assert!(
        reached_into_cmd.is_empty(),
        "the terminal verbs are named outside the file that dispatches argv: {reached_into_cmd:?}. \
         The bot and the hub read inbound content, and inbound content selects from what the \
         machine knows — it never reaches the verbs that change it."
    );

    // 2. The registry hands out no way to change a project except through its own methods, each
    //    of which holds the lock and writes the file. A `&mut Project` escaping would be a second
    //    setter with no name for the grep above to catch — in ANY visibility, since the hub is
    //    the same crate as the registry and `pub(crate)` is the idiom this crate already uses.
    let registry = read("src/registry.rs");
    let struct_start = registry
        .find("pub struct Registry {")
        .expect("the registry struct is where it was");
    let struct_end = registry[struct_start..]
        .find("\n}")
        .expect("the struct closes")
        + struct_start;
    let fields: Vec<&str> = registry[struct_start..struct_end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty())
        .collect();
    assert_eq!(
        fields,
        vec!["path: PathBuf,", "projects: BTreeMap<ProjectId, Project>,"],
        "the registry's fields changed, and a new one may be a new way to a project: {fields:?}"
    );
    for sig in signatures(registry) {
        assert!(
            !sig.contains("&mut Project")
                && !sig.contains("-> &mut")
                && !sig.contains("Option<&mut")
                && !sig.contains("get_mut")
                && !sig.contains("values_mut")
                && !sig.contains("iter_mut"),
            "the registry hands out a mutable project: `{sig}`"
        );
    }
    //    Nor may an `impl` block for it appear anywhere else — Rust lets any file in the crate add
    //    methods to the type, and a method added beside the relay would be as private as the
    //    struct's own.
    let mut impl_elsewhere = Vec::new();
    for (path, src) in &sources {
        if path == "src/registry.rs" || path.ends_with("/tests.rs") {
            continue;
        }
        for line in shipped(src).lines() {
            let l = line.trim_start();
            if l.starts_with("impl") && l.contains("Registry") {
                impl_elsewhere.push(format!("{path}: {l}"));
            }
        }
    }
    assert!(
        impl_elsewhere.is_empty(),
        "an impl block mentions the registry outside registry.rs: {impl_elsewhere:?}"
    );

    // 3. The bot's command set is closed, and it is two. `bot.rs` pins the names in its own test;
    //    this pins them from the outside, so a third variant cannot be added together with an
    //    edit to that test's expected list in one commit and pass. EVERY variant line counts —
    //    `Allow(String),` and `Allow {` are exactly the shapes an `/allow <user>` would take, and
    //    were exactly the shapes a filter on "no parenthesis" let through.
    let bot = read("src/bot.rs");
    let start = bot
        .find("pub enum Command {")
        .expect("the command enum is where it was");
    let end = bot[start..].find("\n}").expect("the enum closes") + start;
    let variants: Vec<&str> = bot[start..end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
        .collect();
    assert_eq!(
        variants,
        vec!["Projects,", "Help,"],
        "the command set grew: {variants:?}. Letting a person speak is a decision made at a \
         keyboard, never from a message."
    );

    // 4. And the terminal is where the verbs are: argv reaches the setter through `cmd/`, and the
    //    setter is really there to reach.
    assert!(
        registry.contains("pub fn set_may_speak("),
        "the setter moved or was renamed; update the name this guard greps for"
    );
    assert!(
        read("src/cmd/enroll.rs").contains("set_may_speak("),
        "the terminal verb no longer calls the setter"
    );
}

//! There is no mode, no flag, and no path from a Telegram message to a keystroke.
//!
//! For one day there was a flag. `serve` started two things — the hub, and an older path that read
//! rendered panes and typed into them — and `HERDR_TG_PANES` chose between them. That was the wrong
//! instrument: the two did not differ in configuration, they differed in what they were allowed to
//! do to the machine, and a boolean is not how you express that.
//!
//! So the second one was deleted. These tests pin the deletion, because the failure they guard
//! against is not a compile error: someone restores a file, or reads the flag back in, and the
//! binary quietly grows a keyboard again.
//!
//! The rule that a write RPC may not be NAMED anywhere lives in `herdr-client`'s
//! `no_live_write_call_site.rs`. This file guards the layer above it — the modules and the switch.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel is two levels below the repo root")
        .to_path_buf()
}

/// Every `.rs`, `.sh`, `.toml`, `.service` and `.md` this repo ships, skipping build output and the
/// agent scratchpad (which is inside the repo because TMPDIR resolves relative to it here).
fn shipped_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | "node_modules" | "%h") {
                    continue;
                }
                stack.push(p);
            } else if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("rs" | "sh" | "toml" | "service" | "timer" | "json" | "ts")
            ) {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn the_pane_flag_is_gone_rather_than_merely_off() {
    // Off is a state someone can change. Gone is not. If this name reappears anywhere — a unit, a
    // script, a config example — it means the two-modes shape is being rebuilt.
    let root = repo();
    let mut found = Vec::new();
    for f in shipped_files(&root) {
        let Ok(src) = std::fs::read_to_string(&f) else {
            continue;
        };
        // This file names it in prose on purpose; everything else must not.
        if f.ends_with("there_is_no_way_from_telegram_to_a_keyboard.rs") {
            continue;
        }
        if src.contains("HERDR_TG_PANES") {
            found.push(f.strip_prefix(&root).unwrap_or(&f).display().to_string());
        }
    }
    assert!(
        found.is_empty(),
        "the pane flag is back in: {found:?}. It was removed because a boolean was choosing \
         between two products, not two configurations — one that can type into your terminals and \
         one that cannot."
    );
}

#[test]
fn the_modules_that_could_type_are_not_declared_again() {
    // A file restored on disk is caught by `no_live_write_call_site.rs` the moment it names a write
    // method. A module DECLARED but empty would not be, and would be the first step back.
    // The crate root is `lib.rs` since the rename gave the crate two binaries; it is where a `mod`
    // declaration has to land to bring one of these back.
    let root = std::fs::read_to_string(repo().join("crates/kickoff-channel/src/lib.rs"))
        .expect("lib.rs is readable");
    for gone in ["mod deliver;", "mod permission;", "mod mirror;"] {
        assert!(
            !root.contains(gone),
            "lib.rs declares `{gone}` again. Those modules read rendered terminals and sent \
             keystrokes; they were deleted because five review rounds could not make that safe."
        );
    }
}

#[test]
fn neither_way_of_serving_takes_a_herdr_connection() {
    // The signatures are the evidence. The bot needed a herdr client only to watch panes and type
    // into them; a way of serving that took one again would mean something had been rebuilt.
    //
    // There are two of them now — the phone line and the app — and BOTH are named here on
    // purpose. A guard that watched one while the other was free to grow a herdr client would
    // watch the wrong half the moment the second became the one that ships.
    let bot = std::fs::read_to_string(repo().join("crates/kickoff-channel/src/bot.rs"))
        .expect("bot.rs is readable");
    assert!(
        bot.contains("pub async fn serve_telegram(config: Config)"),
        "the phone line's signature changed. It takes the config and nothing else: its inputs are \
         Telegram and its own socket."
    );
    assert!(
        bot.contains("pub async fn serve_the_app()"),
        "the app plane's signature changed. It takes nothing at all: its one input is its own \
         socket."
    );
    assert!(
        !bot.contains("HerdrClient"),
        "bot.rs mentions HerdrClient again. The bot does not talk to herdr; the read-only \
         subcommands do."
    );
}

//! The product name may move without making an adopter move all at once.
//!
//! This is the source-only seam of the `herdr-tg` -> `kickoff-channel` rename. The new name is
//! what a fresh adopter sees and invokes, while the old command remains an honest alias for the
//! units and scripts already installed on a box. State and configuration compatibility are pinned
//! in the modules that resolve them; this test pins the package and executable surface Cargo ships.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the channel crate sits two levels below the workspace root")
        .to_path_buf()
}

fn version(binary: &str) -> String {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("could not run {binary}: {error}"));
    assert!(
        output.status.success(),
        "{binary} --version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("version output is UTF-8")
}

#[test]
fn cargo_ships_the_new_command_and_keeps_the_old_one_as_an_alias() {
    // `env!` rather than `option_env!`: Cargo defines these for every integration test, and a
    // build that did not define them has not built the two commands this test exists to pin —
    // failing to compile is the honest outcome, not a panic with a message after the fact.
    let canonical = env!("CARGO_BIN_EXE_kickoff-channel");
    let legacy = env!("CARGO_BIN_EXE_herdr-tg");

    assert!(version(canonical).starts_with("kickoff-channel "));
    assert!(version(legacy).starts_with("kickoff-channel "));
}

#[test]
fn the_current_readme_leads_with_the_new_product_name() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("README.md is readable");
    assert!(
        readme.starts_with("# Kickoff Channel\n"),
        "README.md must introduce the current product, not its former repository name"
    );
}

// ───────────── the sentences a person reads name the command he can type ─────────────

/// The former name, as a sentence would spell it.
const FORMER: &str = "herdr-tg";

/// Where this product's own sentences live: the binary, the channel plugin, the adapter — and the
/// worked example under `docs/`, which is a program that PRINTS and was read by nothing here.
///
/// The example is the surface a stranger copies: `docs/examples/attach-from-the-document.ts` is an
/// adapter written from `docs/ATTACHING.md` alone, and it told whoever ran it to type a command
/// this product no longer ships. It imports nothing of ours on purpose, which is why it sits under
/// `docs/` — and why a guard that walked only the crates and the adapters never saw it.
///
/// `deploy/` and `scripts/` are deliberately absent. A unit file and an installer name units,
/// files and the alias command on purpose, and a guard that could not tell those apart from prose
/// would have to be argued with on every install change rather than read.
const SURFACES: [&str; 5] = [
    "crates/kickoff-channel/src",
    "crates/kickoff-channel/tests",
    "plugins/kickoff-channel",
    "adapters/kickoff-hub-attach",
    "docs/examples",
];

/// Every sentence this product prints must name the command a person can actually run.
///
/// The rename moved the directory, the units, the scripts and the documents, and left the
/// highest-traffic surface of all behind: the lines the binary, the bridge and the adapter PRINT.
/// Two names in two places is how somebody ends up running the stale binary, which has happened on
/// the operator's own box twice — he reads "run herdr-tg enroll", types it, and reaches whatever
/// copy of the old command is first on his PATH instead of the build the documents describe.
///
/// What this does NOT forbid is the former name as part of a NAME: `~/.config/herdr-tg/env`,
/// `herdr-tg.toml`, `deploy/herdr-tg-watchdog.sh`, the state directory, and the `comm` value the
/// kernel reports for the alias. Those are things on this machine that really are still called
/// that, and renaming them would strand a box rather than inform anybody.
#[test]
fn every_sentence_this_product_prints_names_the_command_it_ships() {
    let root = workspace_root();
    let mut complaints: Vec<String> = Vec::new();

    for surface in SURFACES {
        let dir = root.join(surface);
        assert!(
            dir.is_dir(),
            "{surface} is not a directory — this guard would pass by reading nothing"
        );
        for file in files_under(&dir, &["rs", "ts"]) {
            let source = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("could not read {}: {e}", file.display()));
            let shown = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            for (line, text) in names_the_former_command(&what_a_person_reads(&source)) {
                complaints.push(format!("{shown}:{line}: {text}"));
            }
        }
    }

    assert!(
        complaints.is_empty(),
        "these lines tell a person to type, or run, a name the product no longer calls itself. \
         Say `kickoff-channel` — the command the documents and the units now name — or, where the \
         sentence is about a build rather than a command, say which build and drop the name:\n\n{}",
        complaints.join("\n")
    );
}

/// Every file of the named kinds under a directory, ignoring somebody else's code.
fn files_under(dir: &Path, kinds: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(here) = todo.pop() {
        let entries = std::fs::read_dir(&here)
            .unwrap_or_else(|e| panic!("could not list {}: {e}", here.display()));
        for entry in entries {
            let entry = entry.expect("a directory entry");
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Dependencies are not this product's sentences, and `node_modules` is most of the
                // bytes under the plugin.
                if name != "node_modules" && name != "target" {
                    todo.push(path);
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| kinds.contains(&e))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Blank out the lines nobody is ever shown, keeping the line numbering.
///
/// **A `///` on a clap item is not a comment — it is the help text.** `clap` prints the doc comment
/// on a command, an argument or a field verbatim, as the paragraph the operator reads when he asks
/// what a verb does or types one wrong, so it is a printed sentence that happens to be spelled like
/// a comment. The first version of this guard blanked every line beginning `//`, which includes
/// `///`, and so read none of them: the whole of `kickoff-channel --help` was invisible to the one
/// guard whose job is to stop the product naming a command it no longer ships.
///
/// An ordinary `//` line really is a comment. Comments in this repo explain WHY, and several name
/// the former command on purpose while describing the box it is still installed on, so they stay
/// blanked.
///
/// This is a line-based pass rather than a parser: a `//` inside a string literal truncates the
/// line early, which can only make this guard miss a sentence, never invent one. A guard that
/// over-reports is a guard people learn to argue with.
fn what_a_person_reads(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let help = help_clap_prints(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") {
                return if help.contains(&index) { *line } else { "" };
            }
            if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
                return "";
            }
            match line.find("//") {
                Some(at) => &line[..at],
                None => *line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines `clap` will print at somebody: the doc comment on a type that derives one of its
/// traits, and every doc comment inside that type's body — the command's own description, a
/// subcommand's, an argument's, a value's.
///
/// A doc comment anywhere else documents code for whoever reads it, and is blanked with the rest
/// of the comments. That is the whole of the distinction: WHERE the sentence is decides who reads
/// it, and nothing about how it is spelled can.
///
/// Two limits, said rather than implied. Brace counting steps over comment text, so a `{` in the
/// prose of a doc comment — this repo's help quotes JSON envelopes — cannot close a type early and
/// blind the guard to every field below it. And a `derive` split across several source lines reads
/// here as no derive at all, so the type is skipped rather than half-read; keep it on one line, as
/// `rustfmt` does.
fn help_clap_prints(lines: &[&str]) -> BTreeSet<usize> {
    let mut help = BTreeSet::new();
    let mut doc_block: Vec<usize> = Vec::new();
    let mut derives_clap = false;
    let mut depth: i32 = 0;
    let mut inside = false;

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if inside {
            if trimmed.starts_with("///") {
                help.insert(index);
                continue;
            }
            depth += net_braces(line);
            if depth <= 0 {
                inside = false;
                depth = 0;
            }
            continue;
        }
        if trimmed.starts_with("///") {
            doc_block.push(index);
            continue;
        }
        if trimmed.starts_with('#') {
            // An attribute sits BETWEEN a doc comment and the item it documents, so it does not
            // end the block — it is how the block learns what it is documenting.
            derives_clap |= names_a_clap_derive(trimmed);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        // The item itself, at last.
        if derives_clap {
            help.extend(doc_block.iter().copied());
            depth = net_braces(line);
            inside = depth > 0;
        }
        doc_block.clear();
        derives_clap = false;
    }
    help
}

/// Does this attribute derive something `clap` reads doc comments for?
fn names_a_clap_derive(attribute: &str) -> bool {
    attribute.starts_with("#[derive(")
        && ["Parser", "Subcommand", "Args", "ValueEnum"]
            .iter()
            .any(|trait_name| attribute.contains(trait_name))
}

/// Braces opened minus braces closed, counting only the code on the line.
fn net_braces(line: &str) -> i32 {
    let code = match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    };
    code.chars()
        .map(|c| match c {
            '{' => 1,
            '}' => -1,
            _ => 0,
        })
        .sum()
}

/// Line numbers, and the line, wherever the former name is used as a word rather than as part of
/// a path, a filename or an identifier.
fn names_the_former_command(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(offset) = line[from..].find(FORMER) {
            let at = from + offset;
            let end = at + FORMER.len();
            let before = line[..at].chars().next_back();
            let mut after = line[end..].chars();
            let next = after.next();
            let then = after.next();
            if !part_of_a_name(before, next, then) {
                // One complaint per line: it is enough to send somebody to the line, and a line
                // that names the command twice is one edit, not two.
                found.push((index + 1, line.trim().to_string()));
                break;
            }
            from = end;
        }
    }
    found
}

/// Is this occurrence part of something that really is still called that?
fn part_of_a_name(before: Option<char>, next: Option<char>, then: Option<char>) -> bool {
    // `CARGO_BIN_EXE_herdr-tg` and friends: an identifier that happens to end in the old name.
    if before == Some('_') {
        return true;
    }
    match next {
        // The name runs on into a path, a filename or a longer identifier — but only if what
        // follows really does continue it. `restart herdr-tg.` ends a sentence; `herdr-tg.toml`
        // names a file. A `{}` counts as continuation: `herdr-tg-{}-{}` is a minted identifier.
        Some('/' | '.' | '-') => matches!(
            then,
            Some(c) if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '{')
        ),
        // An escaped quote inside a literal: `\"herdr-tg\"` is a name in embedded JSON.
        Some('\\') => true,
        // A literal that is EXACTLY the old name is a directory segment, a process name or a
        // fixture, never a sentence.
        Some(quote) if is_quote(quote) => before.is_some_and(is_quote),
        _ => false,
    }
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

// ───────────── the lines a document quotes as this command's own output ─────────────

/// The three registers `--check` prints in, spelled with the spacing that lines its columns up.
///
/// A document that quotes one of these is not describing the command, it is TRANSCRIBING it, and a
/// transcript is a claim about today that a rename can falsify without touching the document.
const REGISTERS: [&str; 3] = ["ok   ", "NOT  ", "warn "];

/// A line a document quotes as this command's output names the command that printed it.
///
/// `docs/ATTACHING.md` §13.4 is a table of what `kickoff-hub-attach --check` prints, and the
/// document says of it, in the same change that moved the sentences: "the lines are quoted below
/// exactly as the command prints them". That was written while the code printed something else —
/// the rename moved `check.ts` and `attach.ts` and left the transcript behind — so the document's
/// own justification had been authored for text the change had already falsified. A stranger
/// implements from that document (`docs/examples/attach-from-the-document.ts` is an adapter built
/// out of it and run against the real door), which makes a transcript there load-bearing in
/// exactly the way a printed string is.
///
/// It shares the name-shape rule with the guard above, deliberately: two halves of one property
/// that disagreed about what counts as the name would be worse than one half.
///
/// # Why this reads transcripts and not prose, though prose tells a stranger what to type
///
/// The honest answer is that the transcript half is decidable and the prose half is not, and a
/// guard that has to be argued with is a guard that gets deleted.
///
/// **A record is not a file — it is a paragraph, and it lives inside the live documents.**
/// `docs/ATTACHING.md` §3 and `docs/CAPABILITIES.md` both carry a version log, and those entries
/// say what a PAST build shipped: "v10 builds offer 8 … `herdr-tg projects --json` ships", "v12 …
/// `herdr-tg allow <repo> <user>`". `docs/CONVERSATIONS.md` carries a step-by-step record of what
/// shipped, and `docs/MULTIPLEXER-READINESS.md` is an audit of a box as it was. Fourteen of the
/// twenty-four prose mentions left under `docs/` are of that kind, sitting in the documents a
/// stranger is sent to read, and every one of them is true as written — a build that shipped under
/// the old name shipped under the old name. So the historical documents a list could name
/// (`SLICE-*`, `HUB-DESIGN`, `HARDENING-HANDOFF`, `PROPOSAL-*`) are not the whole of the history,
/// and a guard scoped by file name would still have to be argued with inside the files it did read.
///
/// **And the name-shape rule is tuned for code, where a path carries on past the name.** In prose
/// a path routinely ENDS with it — `$XDG_STATE_HOME/herdr-tg`, `~/.local/state/herdr-tg` — and the
/// state directory deliberately did not move with the product, so seven more of those twenty-four
/// are sentences that must keep saying it.
///
/// That leaves three, named here rather than held: one sentence in `docs/RUNNING-THE-HUB.md` that
/// is ABOUT the alias and right to name it, and two a prose guard would rightly catch —
/// the row of `docs/HUB-AND-KICKOFF.md`'s table that starts the hub with `herdr-tg serve`, and
/// `docs/TAXONOMY.md`'s quotation of a hint a past build printed. Two sentences is not a rule's
/// worth of argument with twenty-one others.
///
/// A transcript needs neither judgement: no record under `docs/` quotes one (the walk below reads
/// every `.md` there and the count it reports is the proof), and a register is a shape, not a word.
#[test]
fn every_line_a_document_quotes_as_this_commands_output_names_the_command_it_ships() {
    let root = workspace_root();
    let docs = root.join("docs");
    assert!(
        docs.is_dir(),
        "docs/ is not a directory — this guard would pass by reading nothing"
    );

    let mut complaints: Vec<String> = Vec::new();
    let mut transcripts = 0usize;
    for file in files_under(&docs, &["md"]) {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", file.display()));
        let shown = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (line, quoted) in quoted_transcripts(&text) {
            transcripts += 1;
            if !names_the_former_command(&quoted).is_empty() {
                complaints.push(format!("{shown}:{line}: {quoted}"));
            }
        }
    }

    // A guard that reads nothing passes, and this one reads a shape that a rewrite could stop
    // producing without anybody noticing.
    assert!(
        transcripts > 0,
        "not one line under docs/ is quoted as this command's output, so this guard read nothing. \
         Either the transcripts moved, or the command's registers did."
    );

    assert!(
        complaints.is_empty(),
        "these documents quote this command as printing a name it no longer prints, which is a \
         transcript of a command nobody can run. Run it and quote what it actually prints — \
         `kickoff-hub-attach --check` — rather than editing the line by eye:\n\n{}",
        complaints.join("\n")
    );
}

/// Every inline-code span in the text that opens with one of this command's registers, with the
/// line it sits on.
///
/// Inline code, because that is how every one of them is written: a transcript in this repo's
/// documents is quoted in backticks, inside a table cell or a sentence, never as a fenced block.
/// A fenced transcript would read here as prose and pass, which is the direction to fail in.
fn quoted_transcripts(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for span in line.split('`').skip(1).step_by(2) {
            if REGISTERS.iter().any(|register| span.starts_with(register)) {
                found.push((index + 1, span.to_string()));
            }
        }
    }
    found
}

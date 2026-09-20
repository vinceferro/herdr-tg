//! **What `deploy/kickoff-hub-attach@.service` hands to attach, run rather than read.**
//!
//! That unit is the only artifact in this repo that fails at START time. Its `ExecStart` is a
//! `/bin/sh -c` line carrying two conditional expansions, and with `Restart=always` and
//! `RestartSec=5` under it, a line that no longer means what it says is not a red test — it is a
//! worker dying every five seconds, for ever, on a box the operator can only reach from his phone.
//! Nothing else in the workspace would notice: no crate imports a unit file, and systemd is not
//! run by any gate.
//!
//! So this guard runs the line. It reads the unit off disk, expands the two specifiers systemd
//! expands (`%h`, `%i`), splits the command the way systemd splits it, substitutes variables the
//! way systemd substitutes them, and then starts the real `/bin/sh` with a stand-in for attach in
//! attach's place — a script that prints the argv it was handed. What is asserted is therefore what
//! the process would actually receive, not what a reader believes the line means.
//!
//! # The properties
//!
//! 1. A worker whose environment file names no binding starts **exactly** the command it started
//!    before the binding existed: no empty argument, no stray flag. `${VAR:+…}` is what makes that
//!    true; `${VAR}` would hand attach the flag with an empty word after it, attach refuses a flag
//!    with nothing behind it, and every worker that never wanted a binding would be dead.
//! 2. A binding file arrives behind `--opencode-binding-file`, and its generation behind
//!    `--opencode-binding-generation`.
//! 3. A path with a space in it arrives as ONE argument — the inner quotes, which are easy to
//!    "tidy" away.
//! 4. The shell hands itself over with `exec`, because `KillMode=mixed` signals the MAIN pid and
//!    attach is what forwards SIGTERM to the engine and says goodbye. A shell that stays alive is a
//!    shell that eats the signal.
//! 5. Both `ExecStartPre` gates still refuse what they exist to refuse — a missing port, and a
//!    binding path written with a specifier that an environment file expands for nobody.
//!
//! # This guard FAILS when it cannot look
//!
//! The sibling guard in `crates/herdr-client/tests/no_live_write_call_site.rs` was walked past six
//! times, and nearly every one ended the same way: a lookup came up empty and the code quietly
//! carried on. So there is no path here from "found nothing" to green:
//!
//! * A unit file that cannot be read is a failure, not an empty scan.
//! * No `ExecStart`, more than one, or a line continuation this guard does not model is a failure.
//! * A shell that is not on this machine, or is not executable, is a failure — never an argv of
//!   nothing compared against nothing.
//! * A `%` specifier or a `$` construct this guard does not model is a failure, not a literal
//!   copied through. Dropping the `$$` from a conditional is exactly that mistake, and it lands
//!   here as a refusal rather than as a pass.
//! * The stand-in for attach prints its own name first, so "the command never reached attach" can
//!   never read as "attach was handed no arguments".
//!
//! Every mutation test below rewrites a COPY of the unit in a string and requires the spelling it
//! rewrites to appear exactly once — so an edit to the real unit that moves the spelling turns the
//! RED proof red, rather than leaving it mutating nothing and passing.
//!
//! It reads one file and starts `/bin/sh`. It installs nothing, and asks systemd nothing.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The unit under test, workspace-relative.
const UNIT: &str = "deploy/kickoff-hub-attach@.service";

/// Where the unit looks for attach, under whatever `%h` expands to. The stand-in is written here,
/// so a unit that starts something else fails loudly at "never reached attach" instead of being
/// scored against a command nobody ran.
const ATTACH_UNDER_HOME: &str = ".local/bin/kickoff-hub-attach";

/// The instance name this guard pretends to be, so `%i` expands to something a message can name.
const INSTANCE: &str = "oc-dogfood";

/// The port the pretend environment file names. Nothing is bound; it is a string in an argv.
const PORT: &str = "9711";

/// The argv the unit has started since before any binding existed, written out rather than built,
/// because "exactly today's command" is the property and a builder could drift with the guard.
const TODAY: [&str; 7] = [
    "--opencode",
    "http://127.0.0.1:9711",
    "--run",
    "opencode",
    "serve",
    "--port",
    "9711",
];

/// The stand-in for attach: it prints its own name, then every argument, NUL-separated.
///
/// NUL because the whole point is that a path with a space in it stays one argument — a separator
/// that could occur inside an argument would prove nothing. The name first because an empty stdout
/// must never be readable as "attach was called with no arguments".
const STAND_IN_FOR_ATTACH: &str = "#!/bin/sh\nprintf 'kickoff-hub-attach\\0'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\"; done\n";

/// The workspace root: `crates/kickoff-channel/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

/// Read the unit, or say why this guard is blind.
///
/// The `Err` is not something to log and carry on with. A unit this guard cannot open tells it
/// nothing about what that unit starts.
fn read_unit(root: &Path) -> Result<String, String> {
    let path = root.join(UNIT);
    std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{UNIT} could not be read ({e}). This guard proves nothing about a unit it cannot open, \
             so it fails instead of reporting that the command line is fine."
        )
    })
}

/// The repo's own unit. Unreadable is a panic here: every test below is about this file.
fn the_unit() -> String {
    read_unit(&workspace_root()).unwrap_or_else(|why| panic!("{why}"))
}

/// The single `ExecStart=` value, or why there is no single one.
///
/// `ExecStartPre=` does not start with `ExecStart=`, so the gates are not caught by this. A second
/// `ExecStart` is refused rather than guessed at: systemd would start something this guard never
/// looked at.
fn exec_start(unit: &str) -> Result<String, String> {
    settings(unit, "ExecStart=").and_then(|values| match values.len() {
        1 => Ok(values.into_iter().next().expect("just counted one")),
        0 => Err(format!(
            "{UNIT} has no ExecStart. There is then no command to check, and an empty argv must \
             never compare equal to nothing and read as green."
        )),
        n => Err(format!(
            "{UNIT} has {n} ExecStart lines. This guard checks one command; with several, the one \
             it did not look at is the one that would restart-loop."
        )),
    })
}

/// Every `ExecStartPre=` value in order, or why there is none to run.
///
/// Empty is an error, not an empty loop: the gates are the whole reason a mistyped environment file
/// is a refusal at start rather than a worker restarting every five seconds for ever, and a guard
/// that ran no gate would report every bad environment as "refused by nobody, fine".
fn exec_start_pres(unit: &str) -> Result<Vec<String>, String> {
    let values = settings(unit, "ExecStartPre=")?;
    if values.is_empty() {
        return Err(format!(
            "{UNIT} runs nothing before the worker starts. The port check and the binding-path check \
             are what keep a mistyped environment file from becoming a five-second restart loop; if \
             they are gone on purpose, that decision belongs here."
        ));
    }
    Ok(values)
}

/// The values of one setting, in file order.
///
/// A line ending in a backslash is a systemd continuation, and this guard does not model one: it
/// would read half a command and score it. Refused rather than truncated.
fn settings(unit: &str, key: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for (n, raw) in unit.lines().enumerate() {
        let line = raw.trim_start();
        if line.ends_with('\\') {
            return Err(format!(
                "{UNIT}:{} ends in a backslash, which systemd reads as a continued line and this \
                 guard does not model. It would check half a command line and report that half as \
                 green.",
                n + 1
            ));
        }
        if let Some(value) = line.strip_prefix(key) {
            values.push(value.to_string());
        }
    }
    Ok(values)
}

/// `%h` and `%i`, expanded on the raw value the way systemd expands them: when the unit is LOADED,
/// before the command line is split into words.
///
/// Any other specifier is refused. A `%t` or a `%n` copied through as two literal characters is how
/// this guard would score a command that systemd starts differently from the one it ran.
fn expand_specifiers(line: &str, home: &str, instance: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(home),
            Some('i') => out.push_str(instance),
            Some('%') => out.push('%'),
            Some(other) => {
                return Err(format!(
                    "{UNIT} uses the specifier `%{other}`, which this guard does not expand. It \
                     will not pass those two characters through as if they were a path and then \
                     report on a command systemd would start differently."
                ));
            }
            None => {
                return Err(format!("{UNIT} has a command line ending in a bare `%`."));
            }
        }
    }
    Ok(out)
}

/// systemd's own word splitting: whitespace separates, `'` and `"` quote, `\` escapes.
///
/// The whole shell script of an `ExecStart=/bin/sh -c '…'` is ONE word by this rule, which is why
/// the `$$` escapes inside it matter — see [`expand_variables`]. An unterminated quote is refused
/// rather than run to the end of the line.
fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => {
                            return Err(format!(
                                "{UNIT} has a command line with an unclosed single quote. systemd \
                                 refuses to load that unit, so this guard refuses to guess what it \
                                 would have started."
                            ));
                        }
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped) => word.push(escaped),
                            None => {
                                return Err(format!(
                                    "{UNIT} has a command line ending in a backslash inside double \
                                     quotes."
                                ));
                            }
                        },
                        Some(c) => word.push(c),
                        None => {
                            return Err(format!(
                                "{UNIT} has a command line with an unclosed double quote."
                            ));
                        }
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(escaped) => word.push(escaped),
                    None => {
                        return Err(format!("{UNIT} has a command line ending in a backslash."));
                    }
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// systemd's variable substitution, applied to one word after the splitting that removed its
/// quotes — which is the order systemd applies it in, and the reason a `$` inside single quotes is
/// still systemd's `$` and still needs the `$$` escape.
///
/// * `$$` is a literal dollar, left for whatever reads the line after systemd.
/// * `${NAME}` is the exact value, always exactly one argument, empty when unset.
/// * `$NAME` as a word of its OWN is the value split at whitespace: zero arguments when unset.
///
/// Anything else beginning with `$` is refused. `${NAME:+…}` without the `$$` in front of it is the
/// mistake this refusal is for: systemd does not know `:+`, so passing it through as if it did
/// would have this guard scoring a command that no worker ever runs.
fn expand_variables(word: &str, env: &BTreeMap<&str, &str>) -> Result<Vec<String>, String> {
    if let Some(name) = word.strip_prefix('$') {
        if is_name(name) {
            return Ok(env
                .get(name)
                .copied()
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect());
        }
    }

    let mut out = String::new();
    let bytes = word.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            // A whole character, not a byte: the operator's own words are in the gates' messages
            // and one of them could be non-ASCII, and half a character would not be a message.
            let c = word[i..]
                .chars()
                .next()
                .expect("i is on a character boundary");
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        match bytes.get(i + 1) {
            // A literal dollar. Consumed whole, so the `$` it leaves behind is never read back as
            // the start of a variable — `$$OPENCODE_PORT` is for the shell, not for systemd.
            Some(b'$') => {
                out.push('$');
                i += 2;
            }
            Some(b'{') => {
                let close = word[i + 2..]
                    .find('}')
                    .map(|at| i + 2 + at)
                    .ok_or_else(|| {
                        format!("{UNIT} has a `${{` that is never closed in its command line.")
                    })?;
                let name = &word[i + 2..close];
                if !is_name(name) {
                    return Err(format!(
                        "{UNIT} expands `${{{name}}}`, which is not a plain variable name. systemd \
                         is not a shell and knows no `:+`, `:-` or `#`; this guard refuses to \
                         pretend it does rather than score a command no worker would run. A shell \
                         construct belongs behind `$$`, where systemd hands it to /bin/sh intact."
                    ));
                }
                out.push_str(env.get(name).copied().unwrap_or_default());
                i = close + 1;
            }
            Some(_) if is_name_start(bytes[i + 1]) => {
                let end = word[i + 1..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .map(|at| i + 1 + at)
                    .unwrap_or(word.len());
                out.push_str(env.get(&word[i + 1..end]).copied().unwrap_or_default());
                i = end;
            }
            _ => {
                return Err(format!(
                    "{UNIT} has a bare `$` in its command line that is neither `$$`, `${{NAME}}` \
                     nor `$NAME`. This guard refuses it rather than copying it through."
                ));
            }
        }
    }
    Ok(vec![out])
}

/// What a variable name may begin with. A digit does not, which is what keeps `$$1` — the shell's
/// own positional argument — from being read here as a variable systemd would expand.
fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// A plain environment variable name, and nothing shell-shaped.
fn is_name(s: &str) -> bool {
    !s.is_empty()
        && is_name_start(s.as_bytes()[0])
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// One `Exec*` value, turned into the argv systemd would execute.
fn command_line(
    value: &str,
    home: &Path,
    instance: &str,
    env: &BTreeMap<&str, &str>,
) -> Result<Vec<String>, String> {
    let home = home
        .to_str()
        .ok_or_else(|| "the stand-in home is not valid UTF-8".to_string())?;
    let expanded = expand_specifiers(value, home, instance)?;
    let mut argv = Vec::new();
    for word in split_words(&expanded)? {
        argv.extend(expand_variables(&word, env)?);
    }
    Ok(argv)
}

/// Refuse to "run" something that is not there. A missing or non-executable `/bin/sh` must be a
/// failure with its own sentence, never an argv of nothing that happens to satisfy no assertion.
fn runnable(program: &str) -> Result<(), String> {
    let path = Path::new(program);
    if !path.is_absolute() {
        return Err(format!(
            "{UNIT} starts `{program}`, which is not an absolute path. systemd requires one, so \
             this guard will not resolve it against a PATH systemd does not use."
        ));
    }
    let meta = std::fs::metadata(path).map_err(|e| {
        format!(
            "{UNIT} starts `{program}`, and this machine could not look at it ({e}). This guard \
             cannot say what the unit starts without running it, so it fails rather than passing \
             on a command it never ran."
        )
    })?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "{UNIT} starts `{program}`, which is not an executable file on this machine."
        ));
    }
    Ok(())
}

/// Really run one command line, with only the environment a worker would have.
///
/// `env_clear` on purpose: an `OPENCODE_*` variable that happened to be exported into `cargo test`
/// would otherwise decide what this guard sees, and the result would depend on whose shell ran it.
fn run(
    argv: &[String],
    env: &BTreeMap<&str, &str>,
    home: &Path,
) -> Result<std::process::Output, String> {
    let program = argv
        .first()
        .ok_or_else(|| format!("{UNIT} has an empty command line."))?;
    runnable(program)?;
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin");
    for (name, value) in env {
        command.env(name, value);
    }
    command
        .output()
        .map_err(|e| format!("`{program}` could not be started ({e})."))
}

/// The environment file a worker of this instance would have, as systemd would hand it on.
fn environment<'a>(lines: &[(&'a str, &'a str)]) -> BTreeMap<&'a str, &'a str> {
    lines.iter().copied().collect()
}

/// The argv attach really receives when this unit starts with this environment file.
fn argv_attach(unit: &str, lines: &[(&str, &str)]) -> Result<Vec<String>, String> {
    let home = tempfile::tempdir().map_err(|e| format!("no temp dir for a stand-in home ({e})"))?;
    let stand_in = home.path().join(ATTACH_UNDER_HOME);
    std::fs::create_dir_all(stand_in.parent().expect("the stand-in has a parent"))
        .map_err(|e| format!("could not make a place for the stand-in attach ({e})"))?;
    std::fs::write(&stand_in, STAND_IN_FOR_ATTACH)
        .map_err(|e| format!("could not write the stand-in attach ({e})"))?;
    std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("could not make the stand-in attach executable ({e})"))?;

    let env = environment(lines);
    let argv = command_line(&exec_start(unit)?, home.path(), INSTANCE, &env)?;
    let out = run(&argv, &env, home.path())?;

    let mut printed: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
    let tail = printed.pop();
    let reached = printed.first() == Some(&&b"kickoff-hub-attach"[..]);
    if !out.status.success() || !reached || tail != Some(&[][..]) {
        return Err(format!(
            "the unit's ExecStart never reached attach: it exited {:?} having printed {:?}, with \
             `{}` on stderr. The command line it built was {:?}.",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr).trim(),
            argv
        ));
    }
    printed[1..]
        .iter()
        .map(|arg| {
            std::str::from_utf8(arg)
                .map(str::to_string)
                .map_err(|e| format!("attach was handed an argument that is not UTF-8 ({e})"))
        })
        .collect()
}

/// What the unit's own gates say about this environment file, before any worker starts: the exit
/// status of the first gate that refuses, and what it said on stderr.
fn gates_before_start(unit: &str, lines: &[(&str, &str)]) -> Result<(i32, String), String> {
    let home = tempfile::tempdir().map_err(|e| format!("no temp dir for a stand-in home ({e})"))?;
    let env = environment(lines);
    for gate in exec_start_pres(unit)? {
        let argv = command_line(&gate, home.path(), INSTANCE, &env)?;
        let out = run(&argv, &env, home.path())?;
        let code = out.status.code().ok_or_else(|| {
            format!("a gate of {UNIT} was killed by a signal rather than exiting.")
        })?;
        if code != 0 {
            return Ok((
                code,
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
    }
    Ok((0, String::new()))
}

/// Today's argv with the conditional flags spliced in where the unit puts them: after
/// `--opencode <url>`, before `--run`.
fn attach_argv(conditional: &[&str]) -> Vec<String> {
    let mut want = vec!["--opencode".to_string(), format!("http://127.0.0.1:{PORT}")];
    want.extend(conditional.iter().map(|w| w.to_string()));
    want.extend(
        ["--run", "opencode", "serve", "--port", PORT]
            .iter()
            .map(|w| w.to_string()),
    );
    want
}

/// The environment file of a worker that names only its port.
const ONLY_A_PORT: [(&str, &str); 1] = [("OPENCODE_PORT", PORT)];

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The properties, each written so a mutated COPY of the unit can be put through the same check.

fn check_a_worker_without_a_binding_starts_todays_command(unit: &str) -> Result<(), String> {
    let argv = argv_attach(unit, &ONLY_A_PORT)?;
    if argv != TODAY {
        return Err(format!(
            "a worker whose environment file names no binding started {argv:?}. It has to start \
             exactly {TODAY:?} — an empty argument or a flag with nothing behind it is attach \
             refusing at start, and with Restart=always that is a worker dying every five seconds."
        ));
    }
    Ok(())
}

fn check_an_empty_binding_line_passes_no_flag(unit: &str) -> Result<(), String> {
    let argv = argv_attach(
        unit,
        &[("OPENCODE_PORT", PORT), ("OPENCODE_BINDING_FILE", "")],
    )?;
    if argv != TODAY {
        return Err(format!(
            "an environment file with `OPENCODE_BINDING_FILE=` and nothing after it started \
             {argv:?}. An operator who wrote the line and left it blank meant no binding, so the \
             flag has to disappear whole, as it does for a line that is not there at all."
        ));
    }
    Ok(())
}

fn check_a_binding_file_arrives_behind_its_flag(unit: &str) -> Result<(), String> {
    let path = "/var/lib/kickoff-hub-attach/oc-dogfood.binding";
    let argv = argv_attach(
        unit,
        &[("OPENCODE_PORT", PORT), ("OPENCODE_BINDING_FILE", path)],
    )?;
    let want = attach_argv(&["--opencode-binding-file", path]);
    if argv != want {
        return Err(format!(
            "a worker whose environment file names a binding file started {argv:?}, not {want:?}."
        ));
    }
    Ok(())
}

fn check_a_binding_and_its_generation_both_arrive(unit: &str) -> Result<(), String> {
    let path = "/var/lib/kickoff-hub-attach/oc-dogfood.binding";
    let argv = argv_attach(
        unit,
        &[
            ("OPENCODE_PORT", PORT),
            ("OPENCODE_BINDING_FILE", path),
            ("OPENCODE_BINDING_GENERATION", "7"),
        ],
    )?;
    let want = attach_argv(&[
        "--opencode-binding-file",
        path,
        "--opencode-binding-generation",
        "7",
    ]);
    if argv != want {
        return Err(format!(
            "a worker started for binding 7 started {argv:?}, not {want:?}. The number is what \
             fences a watcher that came back across a restart from taking a stale note as the \
             newest thing it has ever seen."
        ));
    }
    Ok(())
}

fn check_a_path_with_a_space_arrives_as_one_argument(unit: &str) -> Result<(), String> {
    let path = "/var/two words/oc-dogfood.binding";
    let argv = argv_attach(
        unit,
        &[("OPENCODE_PORT", PORT), ("OPENCODE_BINDING_FILE", path)],
    )?;
    let want = attach_argv(&["--opencode-binding-file", path]);
    if argv != want {
        return Err(format!(
            "a binding path with a space in it reached attach as {argv:?}, not {want:?}. Without \
             the quotes inside the conditional the shell splits it, and attach is handed a path \
             that does not exist plus an argument it has no flag for."
        ));
    }
    Ok(())
}

fn check_the_shell_hands_itself_over_to_attach(unit: &str) -> Result<(), String> {
    let value = exec_start(unit)?;
    // A home that cannot exist: nothing is run here, and a specifier still has to expand.
    let words = split_words(&expand_specifiers(&value, "/nonexistent", INSTANCE)?)?;
    let script = match words.as_slice() {
        [shell, dash_c, script, ..] if dash_c == "-c" => {
            runnable(shell)?;
            script.clone()
        }
        _ => {
            return Err(format!(
                "{UNIT}'s ExecStart is no longer a shell with a `-c` script: it is {words:?}. This \
                 guard checks the shell line; it cannot say anything about a command of another \
                 shape."
            ));
        }
    };
    if !script.starts_with("exec ") {
        return Err(format!(
            "{UNIT}'s ExecStart runs `{script}` without `exec`, so /bin/sh stays alive as the main \
             pid. KillMode=mixed signals the MAIN pid only, and attach is what forwards SIGTERM to \
             the engine and says goodbye — a shell in front of it eats the signal and the worker \
             dies at TimeoutStopSec by SIGKILL, silently."
        ));
    }
    if !unit.contains("KillMode=mixed") {
        return Err(format!(
            "{UNIT} no longer says KillMode=mixed. That setting is the whole reason the shell must \
             `exec`; if it changed, the pairing has to be decided again rather than half-kept here."
        ));
    }
    Ok(())
}

fn check_a_worker_without_a_port_is_refused_before_it_starts(unit: &str) -> Result<(), String> {
    let (code, said) = gates_before_start(unit, &[])?;
    if code == 0 {
        return Err(format!(
            "{UNIT} let a worker whose environment file names no port start. `opencode serve` with \
             an empty --port picks a port nobody dialled, so the worker would hold the claim, get \
             its topic, and deliver nothing."
        ));
    }
    if !said.contains("OPENCODE_PORT") {
        return Err(format!(
            "the gate refused a worker with no port but said `{said}`, which does not name the \
             variable the operator has to put in his environment file."
        ));
    }
    Ok(())
}

fn check_a_path_only_a_unit_file_could_expand_is_refused(unit: &str) -> Result<(), String> {
    let (code, said) = gates_before_start(
        unit,
        &[
            ("OPENCODE_PORT", PORT),
            (
                "OPENCODE_BINDING_FILE",
                "%h/.local/state/kickoff-hub-attach/%i.binding",
            ),
        ],
    )?;
    if code == 0 {
        return Err(format!(
            "{UNIT} let a worker start with a binding path written in specifiers. An environment \
             file is not a unit file: `%h` and `%i` reach attach as those four characters, attach \
             refuses a path that is not absolute, and with Restart=always that is a worker dying \
             every five seconds over one line the operator was told to write."
        ));
    }
    if !said.contains("full path") {
        return Err(format!(
            "the gate refused an unexpandable binding path but said `{said}`, which does not tell \
             the operator to write the path out in full."
        ));
    }
    Ok(())
}

fn check_an_ordinary_environment_file_passes_every_gate(unit: &str) -> Result<(), String> {
    for lines in [
        ONLY_A_PORT.to_vec(),
        vec![
            ("OPENCODE_PORT", PORT),
            ("OPENCODE_BINDING_FILE", "/var/lib/oc-dogfood.binding"),
        ],
    ] {
        let (code, said) = gates_before_start(unit, &lines)?;
        if code != 0 {
            return Err(format!(
                "{UNIT} refused an ordinary environment file {lines:?} before the worker started, \
                 saying `{said}`. A gate that refuses what it was meant to allow is the restart \
                 loop it exists to prevent."
            ));
        }
    }
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The properties, against the unit in this tree.

#[test]
fn a_worker_that_names_no_binding_starts_exactly_the_command_it_started_before_bindings_existed() {
    check_a_worker_without_a_binding_starts_todays_command(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn an_environment_file_that_names_the_binding_and_leaves_it_empty_passes_no_flag_at_all() {
    check_an_empty_binding_line_passes_no_flag(&the_unit()).unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_worker_whose_environment_file_names_a_binding_file_hands_that_path_to_attach_behind_the_flag()
{
    check_a_binding_file_arrives_behind_its_flag(&the_unit()).unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_worker_started_for_a_binding_generation_hands_attach_both_the_note_and_the_number() {
    check_a_binding_and_its_generation_both_arrive(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_binding_path_with_a_space_in_it_reaches_attach_as_one_argument() {
    check_a_path_with_a_space_arrives_as_one_argument(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn the_shell_replaces_itself_with_attach_so_a_stop_signal_reaches_what_says_goodbye() {
    check_the_shell_hands_itself_over_to_attach(&the_unit()).unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_worker_whose_environment_file_names_no_port_is_refused_before_it_starts() {
    check_a_worker_without_a_port_is_refused_before_it_starts(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_binding_path_an_environment_file_cannot_expand_is_refused_before_the_worker_starts() {
    check_a_path_only_a_unit_file_could_expand_is_refused(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn an_environment_file_naming_a_port_and_a_real_path_passes_every_gate_the_unit_runs_first() {
    check_an_ordinary_environment_file_passes_every_gate(&the_unit())
        .unwrap_or_else(|why| panic!("{why}"));
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The guard's own guards. A check nobody attacked is a check that reports green.
//
// Every one of these mutates a COPY of the unit in memory. Nothing under `deploy/` is written.

/// The unit with one spelling rewritten, refusing to mutate nothing.
///
/// The count is the point: a RED proof whose mutation silently matched nothing is a test that
/// passes for the wrong reason for ever after. If an edit to the unit moves the spelling, this
/// panics here rather than quietly proving that an unmutated unit still behaves.
fn the_unit_with(from: &str, to: &str) -> String {
    let unit = the_unit();
    let found = unit.matches(from).count();
    assert_eq!(
        found, 1,
        "this RED proof rewrites `{from}`, which appears {found} times in {UNIT} rather than once. \
         Point it at what the unit says now: a mutation that matches nothing proves nothing."
    );
    unit.replace(from, to)
}

/// The unit with one `Exec*` line taken out, refusing to remove nothing.
fn the_unit_without_the_line_that_mentions(marker: &str) -> String {
    let unit = the_unit();
    let hits = unit
        .lines()
        .filter(|l| l.trim_start().starts_with("Exec") && l.contains(marker))
        .count();
    assert_eq!(
        hits, 1,
        "this RED proof removes the Exec line mentioning `{marker}`, and {hits} lines match rather \
         than one."
    );
    unit.lines()
        .filter(|l| !(l.trim_start().starts_with("Exec") && l.contains(marker)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_unit_that_cannot_be_read_fails_instead_of_finding_nothing() {
    let nowhere = tempfile::tempdir().expect("a temp dir");
    let why = read_unit(nowhere.path()).expect_err("a unit that is not there was read anyway");
    assert!(
        why.contains("could not be read"),
        "a missing unit was reported as `{why}`"
    );
}

#[test]
fn a_unit_with_no_exec_start_fails_instead_of_scoring_an_argv_of_nothing() {
    let gutted = the_unit_without_the_line_that_mentions("kickoff-hub-attach --opencode");
    let why = check_a_worker_without_a_binding_starts_todays_command(&gutted)
        .expect_err("a unit that starts nothing was scored as starting today's command");
    assert!(
        why.contains("no ExecStart"),
        "a unit with no ExecStart was reported as `{why}`"
    );
}

#[test]
fn a_unit_whose_shell_is_not_on_this_machine_fails_instead_of_reporting_a_command_it_never_ran() {
    let gone = the_unit_with("ExecStart=/bin/sh -c", "ExecStart=/nonexistent/sh -c");
    let why = check_a_worker_without_a_binding_starts_todays_command(&gone)
        .expect_err("a unit whose shell is not there was scored as if it had run");
    assert!(
        why.contains("could not look at it"),
        "a missing shell was reported as `{why}`"
    );
}

#[test]
fn a_specifier_this_guard_does_not_expand_is_refused_rather_than_copied_through() {
    let strange = the_unit_with(
        "%h/.local/bin/kickoff-hub-attach",
        "%t/.local/bin/kickoff-hub-attach",
    );
    let why = check_a_worker_without_a_binding_starts_todays_command(&strange)
        .expect_err("a specifier this guard cannot expand was passed through as two characters");
    assert!(
        why.contains("%t"),
        "an unmodelled specifier was reported as `{why}`"
    );
}

#[test]
fn a_conditional_expansion_written_as_a_plain_one_hands_attach_an_empty_argument() {
    // The exact mistake the `${VAR:+…}` in the unit exists to prevent: `${VAR}` expands to a single
    // EMPTY argument when the variable is unset, so every worker that never wanted a binding is
    // handed a flag with nothing behind it and refuses to start.
    let plain = the_unit_with(
        "$${OPENCODE_BINDING_FILE:+--opencode-binding-file \"$$OPENCODE_BINDING_FILE\"}",
        "--opencode-binding-file \"$$OPENCODE_BINDING_FILE\"",
    );
    let why = check_a_worker_without_a_binding_starts_todays_command(&plain)
        .expect_err("a worker handed an empty --opencode-binding-file was scored as green");
    assert!(
        why.contains("--opencode-binding-file") && why.contains("\"\""),
        "the empty argument was reported as `{why}`"
    );
}

#[test]
fn a_conditional_that_stopped_treating_an_empty_line_as_no_line_is_caught() {
    // `${VAR+…}` and `${VAR:+…}` differ in exactly one case: a variable that is set and empty. An
    // operator who wrote `OPENCODE_BINDING_FILE=` and stopped meant no binding, not an empty path.
    let no_colon = the_unit_with("{OPENCODE_BINDING_FILE:+", "{OPENCODE_BINDING_FILE+");
    let why = check_an_empty_binding_line_passes_no_flag(&no_colon)
        .expect_err("an empty binding line was allowed to put a flag on the command line");
    assert!(
        why.contains("--opencode-binding-file"),
        "the flag from an empty line was reported as `{why}`"
    );
}

#[test]
fn a_binding_path_that_stopped_being_quoted_reaches_attach_as_two_arguments() {
    let unquoted = the_unit_with(
        "--opencode-binding-file \"$$OPENCODE_BINDING_FILE\"}",
        "--opencode-binding-file $$OPENCODE_BINDING_FILE}",
    );
    // A path without a space still arrives intact, which is why the space is what this is tested
    // with: the quotes look removable right up until an operator has one in a path.
    check_a_binding_file_arrives_behind_its_flag(&unquoted)
        .expect("an unquoted path with no space in it should still arrive whole");
    let why = check_a_path_with_a_space_arrives_as_one_argument(&unquoted)
        .expect_err("a path split at its space was scored as one argument");
    assert!(
        why.contains("\"/var/two\"") && why.contains("\"words/oc-dogfood.binding\""),
        "the split path was reported as `{why}`"
    );
}

#[test]
fn a_shell_that_no_longer_hands_itself_over_is_caught() {
    let lingering = the_unit_with(
        "'exec %h/.local/bin/kickoff-hub-attach",
        "'%h/.local/bin/kickoff-hub-attach",
    );
    let why = check_the_shell_hands_itself_over_to_attach(&lingering)
        .expect_err("a shell that stays in front of attach was scored as green");
    assert!(
        why.contains("without `exec`"),
        "a missing exec was reported as `{why}`"
    );
}

#[test]
fn a_kill_mode_that_no_longer_signals_attach_alone_is_caught() {
    let control_group = the_unit_with("KillMode=mixed", "KillMode=control-group");
    let why = check_the_shell_hands_itself_over_to_attach(&control_group)
        .expect_err("a KillMode that changes what `exec` is for was scored as green");
    assert!(
        why.contains("KillMode=mixed"),
        "the changed KillMode was reported as `{why}`"
    );
}

#[test]
fn a_unit_that_stopped_refusing_a_binding_path_no_environment_file_can_expand_is_caught() {
    let ungated =
        the_unit_without_the_line_that_mentions("OPENCODE_BINDING_FILE must be a full path");
    let why = check_a_path_only_a_unit_file_could_expand_is_refused(&ungated)
        .expect_err("a unit that lets an unexpandable path through was scored as green");
    assert!(
        why.contains("specifiers"),
        "the missing gate was reported as `{why}`"
    );
}

#[test]
fn a_unit_that_stopped_refusing_a_worker_with_no_port_is_caught() {
    let ungated = the_unit_without_the_line_that_mentions("OPENCODE_PORT is not set");
    let why = check_a_worker_without_a_port_is_refused_before_it_starts(&ungated)
        .expect_err("a unit that lets a port-less worker start was scored as green");
    assert!(
        why.contains("names no port"),
        "the missing gate was reported as `{why}`"
    );
}

#[test]
fn a_unit_with_no_gates_at_all_fails_instead_of_running_none_and_reporting_green() {
    let unit = the_unit();
    let gateless: String = unit
        .lines()
        .filter(|l| !l.trim_start().starts_with("ExecStartPre="))
        .collect::<Vec<_>>()
        .join("\n");
    let why =
        gates_before_start(&gateless, &ONLY_A_PORT).expect_err("a unit with no gates ran cleanly");
    assert!(
        why.contains("runs nothing before the worker starts"),
        "a gateless unit was reported as `{why}`"
    );
}

#[test]
fn a_shell_construct_systemd_would_never_expand_is_refused_rather_than_scored() {
    // Dropping the `$$` is the plausible tidy-up: the line then reads like shell, but systemd
    // expands `${…}` itself, knows nothing of `:+`, and hands /bin/sh something else entirely.
    let unescaped = the_unit_with("$${OPENCODE_BINDING_FILE:+", "${OPENCODE_BINDING_FILE:+");
    let why = check_a_worker_without_a_binding_starts_todays_command(&unescaped)
        .expect_err("an expansion systemd would not perform was scored as if it had");
    assert!(
        why.contains("not a plain variable name"),
        "the unescaped construct was reported as `{why}`"
    );
}

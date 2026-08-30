//! Recognising a choice dialog in a pane, and driving it with the right keys.
//!
//! # The bug this exists to close
//!
//! An agent asking *"Access external directory /srv/example-data? — Allow once · Allow always
//! · Reject"* is `blocked`, exactly like an agent asking a question in prose. But it cannot be
//! answered with prose. The dialog captures keys, so text sent to it goes nowhere, and the `Enter`
//! that follows confirms **whatever option happens to be highlighted** — which is the leftmost one,
//! `Allow once`.
//!
//! So a reply of "no" would have granted the permission. That is the catastrophic failure D3 exists
//! to prevent, arriving through a path nobody designed. A dialog is therefore never answered with
//! text: it is answered with buttons that carry the literal options, and a tap sends keys.
//!
//! # Why the ANSI read rather than the text read
//!
//! The plain-text read shows *which options exist* but not *which one is selected* — the highlight
//! is colour, and colour is exactly what `format: "text"` strips. Without the selected index the
//! bridge would have to guess how many arrow presses to send, and a wrong guess confirms the wrong
//! option. So the dialog is parsed from `format: "ansi"`, where the selection is visible.
//!
//! # Why the highlight is found structurally, not by colour value
//!
//! opencode paints the selected option on `48;2;255;225;77`. Matching that literal would bind this
//! parser to one harness's theme and break the day the operator changed it. Instead: the row is
//! painted in the modal's own background, and the selected option is the one painted differently.
//! That holds for any sane selector in any theme, and — unlike the rule this module shipped with,
//! "exactly one background occurs exactly once" — it holds for a dialog with only two options,
//! where each background occurs exactly once and there is no odd one out to find.
//!
//! # A control we cannot read is not prose
//!
//! A dialog that stops being recognised used to fall back to being treated as prose, which is the
//! dangerous path: words go into a modal that swallows them, and the confirm key after them
//! presses whatever is highlighted. So the answer is three-way, not two — see [`Screen`]. A row
//! that is shaped like a control but whose highlight cannot be resolved is refused, and the
//! operator is told to answer it at the keyboard. A false refusal costs them one trip to their
//! terminal; a false "this is prose" grants a permission they never gave.
//!
//! # This bridge is not the only thing driving the dialog
//!
//! The whole point of the product is the operator at their laptop with their phone in their hand,
//! so their own keyboard is answering the same dialog. Worse, the screen this module reads lags
//! that keyboard by tens of milliseconds, so a read taken just after they moved the highlight
//! renders the frame from before it. The selection in any single read may therefore already be
//! wrong.
//!
//! That is why moving and confirming are separate here: [`Prompt::move_to`] emits arrows and never
//! the confirm key, and [`CONFIRM`] travels on its own after a settled re-read has shown the
//! wanted option highlighted. `deliver::choose` owns that sequence. Nothing in this module may
//! hand a caller a batch it cannot look inside.

/// A choice dialog found in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The options, left to right, exactly as rendered.
    pub options: Vec<String>,
    /// Which one is highlighted right now.
    pub selected: usize,
}

/// The key that confirms a choice.
///
/// Hardcoded on purpose: this is NOT the operator's configured submit key. The captured dialog's
/// own footer reads "enter confirm", and routing a config value here would let a mis-set key — the
/// interrupt form among them, which herdr accepts (`docs/SLICE-3-PROBE.md` P2) — be delivered into
/// a focused modal.
pub const CONFIRM: &str = "Enter";

impl Prompt {
    /// The keys that move the highlight from where THIS read saw it to `target`.
    ///
    /// Never includes the confirm key. Moving and confirming go out separately because the only
    /// defence against a screen that lagged the operator's own keypresses is to look again in
    /// between — and there is no such moment if the two travel together.
    ///
    /// `None` if `target` is out of range; empty when the highlight is already there.
    ///
    /// Never wraps, so the highlight is never driven past an end and nothing here depends on what
    /// the harness does at one. That has NOT been probed: `docs/SLICE-3-PROBE.md` P2 settles which
    /// key NAMES herdr accepts and says nothing about what a TUI does with them at an edge.
    pub fn move_to(&self, target: usize) -> Option<Vec<&'static str>> {
        if target >= self.options.len() {
            return None;
        }
        let step = if target > self.selected {
            "Right"
        } else {
            "Left"
        };
        Some(vec![step; target.abs_diff(self.selected)])
    }

    /// The option highlighted right now, as this read saw it.
    pub fn highlighted(&self) -> Option<&str> {
        self.options.get(self.selected).map(String::as_str)
    }

    /// Where `label` sits, or `None` if it is absent or appears more than once.
    ///
    /// Ambiguity is refused rather than resolved by order — the same rule [`Prompt::match_option`]
    /// follows, and for the same reason: on a dialog offering two similar permissions, picking the
    /// first grants the broader one about half the time it was meant to grant the narrower.
    pub fn exact_option(&self, label: &str) -> Option<usize> {
        let hits: Vec<usize> = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.as_str() == label)
            .map(|(i, _)| i)
            .collect();
        (hits.len() == 1).then(|| hits[0])
    }
}

impl Prompt {
    /// Which option does the operator's reply mean?
    ///
    /// Accepts a 1-based number, or a case-insensitive prefix of exactly one option. A reply that
    /// matches two options is refused rather than resolved by order — on a dialog whose options are
    /// `Allow once` and `Allow always`, "allow" is genuinely ambiguous, and picking the first would
    /// grant the broader permission roughly half the time it was meant to grant the narrower one.
    pub fn match_option(&self, reply: &str) -> Option<usize> {
        let r = reply.trim().trim_end_matches(['.', '!']).to_lowercase();
        if r.is_empty() {
            return None;
        }
        if let Ok(n) = r.parse::<usize>() {
            // `then`, not `then_some`: the latter evaluates its argument eagerly, so a reply of
            // "0" underflowed before the bounds check could reject it.
            return (n >= 1 && n <= self.options.len()).then(|| n - 1);
        }
        let exact: Vec<usize> = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.to_lowercase() == r)
            .map(|(i, _)| i)
            .collect();
        if exact.len() == 1 {
            return Some(exact[0]);
        }
        let hits: Vec<usize> = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.to_lowercase().starts_with(&r))
            .map(|(i, _)| i)
            .collect();
        (hits.len() == 1).then(|| hits[0])
    }
}

/// Tokens that end the option row: everything after them is the keybind footer.
const HINT_TOKENS: [&str; 8] = ["ctrl+", "alt+", "⇆", "esc", "enter", "tab", "↑", "↓"];

/// What a pane is showing, as far as this parser can tell.
///
/// Three outcomes, not two, and the middle one is the whole point. A row that is structurally a
/// control but whose highlight cannot be read is NOT prose: words typed at it go nowhere, and the
/// confirm key after them presses whatever happens to be highlighted. The caller must refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// A choice dialog whose options and highlight were both resolved.
    Dialog(Prompt),
    /// Shaped like a control, unreadable. Never write here.
    UnreadableControl,
    /// Ordinary output. The text path is safe.
    Prose,
}

/// Classify an ANSI pane read.
pub fn classify(ansi: &str) -> Screen {
    // BOTTOM-UP. A modal is the last thing drawn, and the agent's own transcript — which it writes,
    // and which can contain the words "select" and "confirm" — sits above it. Taking the first
    // match from the top let one sentence of agent prose hide a live dialog.
    for line in ansi.lines().rev() {
        let Some(options) = option_runs(line) else {
            continue;
        };
        return match selected_index(&options, line) {
            Some(selected) => Screen::Dialog(Prompt {
                options: options.into_iter().map(|(t, _)| t).collect(),
                selected,
            }),
            // The bottom-most control row decides. Looking further up for something resolvable
            // would drive keys at a row that is not the one holding focus.
            None => Screen::UnreadableControl,
        };
    }
    Screen::Prose
}

/// Parse a choice dialog out of an ANSI pane read, or `None` if none could be resolved.
///
/// `None` collapses "ordinary output" and "a control I cannot read" into one answer, so it is safe
/// only for callers that do nothing at all on `None`. Anything deciding whether to WRITE to the
/// pane must use [`classify`], which keeps those two apart.
pub fn parse(ansi: &str) -> Option<Prompt> {
    match classify(ansi) {
        Screen::Dialog(p) => Some(p),
        _ => None,
    }
}

/// The `(text, background)` of the option runs on this row, or `None` if it is not a control row.
///
/// A control row carries the affordance footer AND at least two separately coloured runs before it.
/// Prose that merely mentions the words is one uncoloured run, and is skipped.
fn option_runs(line: &str) -> Option<Vec<(String, String)>> {
    if !is_option_row(line) {
        return None;
    }
    let mut options: Vec<(String, String)> = Vec::new();
    for (sgr, text) in sgr_runs(line) {
        let t = text.trim();
        if t.is_empty() || t.chars().all(|c| "│┃╹▀ ".contains(c)) {
            continue;
        }
        if is_hint(t) {
            break; // the footer starts here
        }
        options.push((t.to_string(), background_of(&sgr)));
    }
    (options.len() >= 2).then_some(options)
}

/// Which option is highlighted?
///
/// Primary rule: the row is painted in the modal's own background — the panel — and the selected
/// option is the one painted differently. That is well defined for TWO options, which "the
/// background that occurs exactly once" is not: with Allow/Deny each background occurs exactly
/// once, and the old rule therefore refused the commonest dialog shape there is.
///
/// Fallback, three options or more: the odd one out. It fires only where the modal rule is
/// inconclusive — a harness that paints the panel in the highlight colour, say — and it is the rule
/// this parser shipped with, so nothing that used to resolve stops resolving.
fn selected_index(options: &[(String, String)], line: &str) -> Option<usize> {
    if let Some(modal) = modal_background(line) {
        let differ: Vec<usize> = options
            .iter()
            .enumerate()
            .filter(|(_, (_, bg))| *bg != modal)
            .map(|(i, _)| i)
            .collect();
        if differ.len() == 1 {
            return Some(differ[0]);
        }
    }
    odd_one_out(options)
}

/// The background covering the most columns of the row, or `None` when two of them tie for it.
///
/// Counted over EVERY run of the row — separators, padding and the keybind footer included —
/// because that is where the panel colour is unambiguous. Counted over the options alone it is not:
/// with two of them there is no majority to find.
fn modal_background(line: &str) -> Option<String> {
    let mut width: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (sgr, text) in sgr_runs(line) {
        *width.entry(background_of(&sgr)).or_default() += text.chars().count();
    }
    let mut ranked: Vec<(String, usize)> = width.into_iter().collect();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.1));
    match ranked.as_slice() {
        [(bg, _)] => Some(bg.clone()),
        [(bg, n), (_, m), ..] if n > m => Some(bg.clone()),
        // No dominant colour means there is no panel to measure against. Refuse rather than guess.
        _ => None,
    }
}

/// The option whose background differs from every other one, when exactly one does.
fn odd_one_out(options: &[(String, String)]) -> Option<usize> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, bg) in options {
        *counts.entry(bg.as_str()).or_default() += 1;
    }
    let unique: Vec<&str> = counts
        .iter()
        .filter(|(_, n)| **n == 1)
        .map(|(bg, _)| *bg)
        .collect();
    if unique.len() != 1 {
        return None;
    }
    options.iter().position(|(_, bg)| bg == unique[0])
}

/// Is this the row carrying the options?
///
/// Requires the affordance footer — a row of words alone is prose, but a row that also tells the
/// user how to select and confirm is a control.
fn is_option_row(line: &str) -> bool {
    let plain = strip_sgr(line).to_lowercase();
    (plain.contains("select") && plain.contains("confirm")) || plain.contains("↑/↓")
}

fn is_hint(t: &str) -> bool {
    let low = t.to_lowercase();
    HINT_TOKENS.iter().any(|h| low.starts_with(h))
        || matches!(low.as_str(), "select" | "confirm" | "fullscreen" | "cancel")
}

/// Split a line into `(sgr-prefix, text)` runs.
fn sgr_runs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut sgr = String::new();
    let mut text = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            if !text.is_empty() {
                out.push((std::mem::take(&mut sgr), std::mem::take(&mut text)));
            }
            chars.next();
            let mut code = String::from("\u{1b}[");
            for c2 in chars.by_ref() {
                code.push(c2);
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
            sgr.push_str(&code);
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        out.push((sgr, text));
    }
    out
}

/// The background portion of an SGR prefix, or an empty string when it sets none.
///
/// Parsed by PARAMETER, never by substring. `38;2;248;250;252` is a stock light foreground and it
/// contains the digits "48;" — a substring search read a background out of it that the terminal
/// never painted, and because the real render emits the foreground first, the phantom won. Two
/// options wearing such a foreground then pointed the highlight at the wrong one.
///
/// The LAST background set wins, which is what makes the foreground-first order safe.
fn background_of(sgr: &str) -> String {
    let mut bg = String::new();
    for cap in sgr.split('\u{1b}') {
        // Each fragment is "[<params>m"; anything else does not set a colour.
        let Some(body) = cap.strip_prefix('[') else {
            continue;
        };
        let Some(body) = body.strip_suffix('m') else {
            continue;
        };
        let params: Vec<&str> = body.split(';').collect();
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match (p, p.parse::<u16>().ok()) {
                // An extended colour carries its operands with it: `2;r;g;b` or `5;n`. They are
                // consumed either way, so a foreground's numbers can never be read as a parameter
                // of their own.
                ("38", _) | ("48", _) => {
                    let take = match params.get(i + 1) {
                        Some(&"2") => 5,
                        Some(&"5") => 3,
                        _ => 2,
                    };
                    let end = (i + take).min(params.len());
                    if p == "48" {
                        bg = params[i..end].join(";");
                    }
                    i = end;
                }
                (_, Some(v)) if (40..=47).contains(&v) || (100..=107).contains(&v) => {
                    bg = v.to_string();
                    i += 1;
                }
                // Reset, the implicit reset of a bare `[m`, and default-background all clear it.
                ("", _) | ("0", _) | ("49", _) => {
                    bg = String::new();
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
    bg
}

fn strip_sgr(s: &str) -> String {
    sgr_runs(s).into_iter().map(|(_, t)| t).collect()
}

/// Synthetic dialog rows, in the captured fixture's own colours.
///
/// Lives beside the parser rather than inside a test module because `deliver`'s tests need to stage
/// a dialog whose highlight moves between reads, which the single captured screen cannot do. A
/// second renderer living over there would drift from this one, and those tests would then pass
/// against screens this parser has never seen.
#[cfg(test)]
pub(crate) mod synthetic {
    /// The selected option's colours in the capture.
    const SEL: &str = "\u{1b}[0m\u{1b}[38;2;10;14;26m\u{1b}[48;2;255;225;77m";
    /// An unselected option's colours in the capture.
    pub(crate) const UNSEL: &str = "\u{1b}[0m\u{1b}[38;2;164;164;164m\u{1b}[48;2;26;37;70m";
    /// The modal panel the whole row sits on.
    pub(crate) const PANEL: &str = "\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;26;37;70m";
    /// The keybind footer, copied out of the capture so a synthetic row ends the way a real one does.
    pub(crate) const FOOTER: &str = "\u{1b}[0m\u{1b}[38;2;232;236;255m\u{1b}[48;2;26;37;70mctrl+f \u{1b}[0m\u{1b}[38;2;164;164;164m\u{1b}[48;2;26;37;70mfullscreen\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;26;37;70m  \u{1b}[0m\u{1b}[38;2;232;236;255m\u{1b}[48;2;26;37;70m\u{21c6} \u{1b}[0m\u{1b}[38;2;164;164;164m\u{1b}[48;2;26;37;70mselect\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;26;37;70m  \u{1b}[0m\u{1b}[38;2;232;236;255m\u{1b}[48;2;26;37;70menter \u{1b}[0m\u{1b}[38;2;164;164;164m\u{1b}[48;2;26;37;70mconfirm\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;26;37;70m   \u{1b}[0m\u{1b}[38;2;255;255;255m  \u{1b}[0m";

    /// A dialog row for the option counts and selections nobody captured.
    ///
    /// A synthetic fixture is worth only what its faithfulness is worth, so `permission`'s own tests
    /// check it against the real capture before anything else leans on it.
    pub(crate) fn render_dialog(options: &[&str], selected: usize) -> String {
        let mut s = String::from(
            "\u{1b}[0m\u{1b}[38;2;255;255;255m  \u{1b}[0m\u{1b}[38;2;255;225;77m\u{1b}[48;2;21;29;55m\u{2503}\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;26;37;70m  ",
        );
        for (i, o) in options.iter().enumerate() {
            if i == selected {
                s.push_str(&format!(
                    "\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;255;225;77m {SEL}{o}\u{1b}[0m\u{1b}[38;2;255;255;255m\u{1b}[48;2;255;225;77m {PANEL}  "
                ));
            } else {
                s.push_str(&format!("{UNSEL}{o}{PANEL}   "));
            }
        }
        s.push_str(FOOTER);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic::*;
    use super::*;

    /// The real dialog, captured from the operator's herd and scrubbed of their paths.
    const REAL: &str = include_str!("../tests/fixtures/opencode-permission.ansi");

    #[test]
    fn the_real_opencode_permission_dialog_parses() {
        let p = parse(REAL).expect("the captured dialog must be recognised");
        assert_eq!(p.options, ["Allow once", "Allow always", "Reject"]);
        assert_eq!(p.selected, 0, "opencode highlights the leftmost option");
    }

    /// THE reason this module exists. `Reject` is two to the right of the default, and a reply of
    /// "no" would previously have confirmed `Allow once`.
    #[test]
    fn choosing_reject_moves_the_highlight_two_to_the_right() {
        let p = parse(REAL).unwrap();
        let reject = p.options.iter().position(|o| o == "Reject").unwrap();
        assert_eq!(p.move_to(reject).unwrap(), ["Right", "Right"]);
    }

    #[test]
    fn choosing_the_already_highlighted_option_moves_nothing() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.move_to(0).unwrap(), Vec::<&str>::new());
    }

    #[test]
    fn moving_left_is_supported_and_never_wraps() {
        let p = Prompt {
            options: vec!["a".into(), "b".into(), "c".into()],
            selected: 2,
        };
        assert_eq!(p.move_to(0).unwrap(), ["Left", "Left"]);
        assert!(
            p.move_to(3).is_none(),
            "an out-of-range target must be refused, not wrapped"
        );
    }

    /// Moving and confirming go out separately, so the bridge has a moment to look before it
    /// commits. A move that carries the confirm key with it leaves no such moment — and the
    /// operator's own keyboard is driving the same dialog.
    #[test]
    fn move_to_never_emits_a_confirm_key() {
        let p = parse(REAL).unwrap();
        for target in 0..p.options.len() {
            for k in p.move_to(target).unwrap() {
                assert!(
                    matches!(k, "Left" | "Right"),
                    "{k} confirms a choice; nothing that only moves the highlight may emit it"
                );
                assert_ne!(k, CONFIRM);
            }
        }
        assert_eq!(
            p.move_to(p.selected).unwrap().len(),
            0,
            "a highlight already where it belongs needs no keys at all"
        );
    }

    /// The direction is decided by where the highlight is, not by the caller.
    #[test]
    fn the_direction_follows_the_highlight() {
        let p = Prompt {
            options: vec!["a".into(), "b".into(), "c".into()],
            selected: 1,
        };
        assert_eq!(p.move_to(2).unwrap(), ["Right"]);
        assert_eq!(p.move_to(0).unwrap(), ["Left"]);
    }

    /// A label that appears twice is refused rather than resolved by order — the same rule
    /// `match_option` follows, and for the same reason.
    #[test]
    fn exact_option_refuses_a_duplicate_label() {
        let p = Prompt {
            options: vec!["Allow".into(), "Deny".into(), "Allow".into()],
            selected: 0,
        };
        assert_eq!(p.exact_option("Deny"), Some(1));
        assert_eq!(
            p.exact_option("Allow"),
            None,
            "two options carry that label"
        );
        assert_eq!(p.exact_option("Maybe"), None);
        // Exact, not a prefix: a button carries the label it displayed, character for character.
        assert_eq!(p.exact_option("deny"), None);
    }

    /// Against the real captured menu: a button's label is matched character for character, which
    /// is what makes it safe to carry one instead of a position.
    #[test]
    fn an_exact_label_resolves_and_a_near_miss_does_not() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.exact_option("Allow once"), Some(0));
        assert_eq!(p.exact_option("Reject"), Some(2));
        assert_eq!(p.exact_option("reject"), None, "the case must match");
        assert_eq!(p.exact_option("Allow"), None, "a prefix is not a label");
        assert_eq!(
            p.exact_option("Reject "),
            None,
            "both sides come out of this parser, which trims, so a stray space is a real difference"
        );
    }

    #[test]
    fn highlighted_names_the_option_this_read_saw_selected() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.highlighted(), Some("Allow once"));
        let out_of_range = Prompt {
            options: vec!["a".into()],
            selected: 7,
        };
        assert_eq!(out_of_range.highlighted(), None);
    }

    /// Every key this module emits must be one herdr accepts on protocol 20. `Left`, `Right` and
    /// `Enter` were all confirmed by the probe; `C-c`-style forms and named navigation keys such as
    /// `Home` were rejected (docs/SLICE-3-PROBE.md P2).
    #[test]
    fn every_emitted_key_is_one_the_probe_confirmed() {
        let p = parse(REAL).unwrap();
        let mut emitted: Vec<&str> = vec![CONFIRM];
        for target in 0..p.options.len() {
            emitted.extend(p.move_to(target).unwrap());
        }
        for k in emitted {
            assert!(
                matches!(k, "Left" | "Right" | "Enter"),
                "{k} was never confirmed against protocol 20"
            );
            assert!(
                herdr_client::Key::parse(k).is_ok(),
                "{k} is not a key the client will accept"
            );
        }
    }

    /// A pane of ordinary agent output must NOT be mistaken for a dialog: buttons on a pane that
    /// ignores them is a worse experience than the text path.
    #[test]
    fn ordinary_output_is_not_a_dialog() {
        assert!(parse("just some text\nand another line").is_none());
        assert!(parse("").is_none());
        // Prose that happens to contain the word "select".
        assert!(parse("I will select the first option and confirm it later").is_none());
    }

    /// An ambiguous render — no single distinct background — must fall through rather than guess.
    /// Guessing here confirms the wrong option.
    #[test]
    fn an_ambiguous_highlight_is_refused_rather_than_guessed() {
        let same = "\u{1b}[48;5;1mAllow once\u{1b}[0m \u{1b}[48;5;1mReject\u{1b}[0m  \u{1b}[0m⇆ select  enter confirm";
        assert!(parse(same).is_none());
        assert_eq!(
            classify(same),
            Screen::UnreadableControl,
            "a control nobody can read must never be reported as ordinary output"
        );
    }

    #[test]
    fn the_footer_is_not_mistaken_for_an_option() {
        let p = parse(REAL).unwrap();
        for o in &p.options {
            assert!(!is_hint(o), "{o} is a keybind hint, not a choice");
        }
        assert!(!p.options.iter().any(|o| o.contains("fullscreen")));
    }

    /// "allow" matches both `Allow once` and `Allow always`. Resolving it by order would grant the
    /// broader permission about half the time it was meant to grant the narrower one.
    #[test]
    fn an_ambiguous_word_is_refused_not_resolved_by_order() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.match_option("allow"), None);
        assert_eq!(p.match_option("Allow once"), Some(0));
        assert_eq!(p.match_option("reject"), Some(2));
        assert_eq!(p.match_option("rej"), Some(2));
    }

    #[test]
    fn a_number_selects_by_position_and_is_bounds_checked() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.match_option("1"), Some(0));
        assert_eq!(p.match_option(" 3 "), Some(2));
        assert_eq!(p.match_option("0"), None);
        assert_eq!(p.match_option("4"), None);
        assert_eq!(p.match_option(""), None);
        assert_eq!(p.match_option("yes please"), None);
    }

    #[test]
    fn background_extraction_handles_truecolor_indexed_and_none() {
        assert_eq!(background_of("\u{1b}[48;2;255;225;77m"), "48;2;255;225;77");
        assert_eq!(background_of("\u{1b}[48;5;33m"), "48;5;33");
        assert_eq!(background_of("\u{1b}[38;2;1;2;3m"), "");
    }

    /// Allow/Deny is the commonest confirm shape there is, and it was structurally invisible: the
    /// old rule looked for one background occurring exactly once, and with two options both do.
    ///
    /// The control comes first, so the synthetic row proves something about the real world before
    /// the new cases lean on it.
    #[test]
    fn a_two_option_dialog_is_recognised_and_the_synthetic_row_is_faithful() {
        assert_eq!(
            parse(&render_dialog(&["Allow once", "Allow always", "Reject"], 0)),
            parse(REAL),
            "the synthetic row must read exactly like the captured one"
        );
        for sel in 0..2 {
            assert_eq!(
                parse(&render_dialog(&["Allow", "Deny"], sel)),
                Some(Prompt {
                    options: vec!["Allow".into(), "Deny".into()],
                    selected: sel,
                })
            );
            assert_eq!(
                parse(&render_dialog(&["Yes", "No"], sel)),
                Some(Prompt {
                    options: vec!["Yes".into(), "No".into()],
                    selected: sel,
                })
            );
        }
    }

    /// The catastrophe this closes: "no" to a Yes/No dialog used to be typed as words, and the
    /// confirm key after them pressed whatever was highlighted — which is Yes.
    #[test]
    fn answering_no_to_a_two_option_dialog_moves_the_selection_instead_of_typing() {
        let p = parse(&render_dialog(&["Yes", "No"], 0)).expect("a Yes/No dialog is a dialog");
        assert_eq!(p.selected, 0, "the highlight starts on Yes");
        assert_eq!(
            p.match_option("no"),
            Some(1),
            "the operator's \"no\" must resolve to the option they named"
        );
        assert_eq!(
            p.move_to(1).unwrap(),
            ["Right"],
            "answering it moves the highlight; it does not type the word"
        );
        assert_eq!(CONFIRM, "Enter", "and the confirm key travels on its own");
    }

    /// `38;2;248;250;252` is a stock light foreground. It contains the digits "48;", and the old
    /// substring search read that as a background — a colour the terminal never painted.
    #[test]
    fn a_truecolor_foreground_is_never_read_as_a_background() {
        assert_eq!(background_of("\u{1b}[38;2;248;250;252m"), "");
        assert_eq!(background_of("\u{1b}[38;2;148;0;211m"), "");
        assert_eq!(
            background_of("\u{1b}[38;2;248;250;252m\u{1b}[48;2;26;37;70m"),
            "48;2;26;37;70",
            "the real render emits the foreground first"
        );
        assert_eq!(background_of("\u{1b}[0;1;44m"), "44");
        assert_eq!(background_of("\u{1b}[44m\u{1b}[49m"), "");
    }

    /// Two options wearing the same light foreground used to look alike, so the third looked like
    /// the odd one out — and a tap on "Reject" then confirmed "Allow once".
    #[test]
    fn two_options_sharing_a_light_foreground_do_not_move_the_highlight() {
        const LIGHT: &str = "\u{1b}[38;2;248;250;252m";
        let line = format!(
            "\u{1b}[0m\u{1b}[38;2;255;255;255m  \u{1b}[0m\u{1b}[38;2;255;225;77m\u{1b}[48;2;21;29;55m\u{2503}{PANEL}  \
             \u{1b}[0m{LIGHT}\u{1b}[48;2;255;225;77mAllow once{PANEL}  \
             \u{1b}[0m{LIGHT}\u{1b}[48;2;26;37;70mAllow always{PANEL}   \
             {UNSEL}Reject{PANEL}   {FOOTER}"
        );
        let p = parse(&line).expect("a dialog painted in stock colours is still a dialog");
        assert_eq!(p.options, ["Allow once", "Allow always", "Reject"]);
        assert_eq!(
            p.selected, 0,
            "the highlighted option is the one on the highlight colour"
        );
    }

    /// The agent writes the transcript above its own dialog, and it can write the words the row
    /// detector looks for. One such sentence used to hide a live dialog completely.
    #[test]
    fn agent_prose_above_a_dialog_does_not_hide_it() {
        let chatty = "I will select the option and confirm it later";
        assert_eq!(parse(&format!("{chatty}\n{REAL}")), parse(REAL));
        assert_eq!(
            parse(&format!("{chatty}\n{chatty}\n{chatty}\n{REAL}")),
            parse(REAL)
        );
    }

    /// The blocker underneath all of this. A row shaped like a control whose highlight cannot be
    /// read must never come back as ordinary output: on ordinary output the bridge types words and
    /// presses the confirm key, and against a modal that presses whatever is highlighted.
    #[test]
    fn an_unreadable_control_row_is_never_classified_as_prose() {
        assert!(matches!(classify(REAL), Screen::Dialog(_)));

        let same_background = "\u{1b}[48;5;1mAllow once\u{1b}[0m \u{1b}[48;5;1mReject\u{1b}[0m  \u{1b}[0m⇆ select  enter confirm";
        assert_eq!(classify(same_background), Screen::UnreadableControl);

        // A theme that marks the selection with the foreground alone leaves nothing to compare.
        let foreground_only = "\u{1b}[1;32mAllow\u{1b}[0m  \u{1b}[2;37mDeny\u{1b}[0m   \u{1b}[0m⇆ select  enter confirm";
        assert_eq!(classify(foreground_only), Screen::UnreadableControl);

        // The other half: ordinary output still gets the ordinary path, or the bridge stops being
        // able to answer an agent at all.
        assert_eq!(classify("just some text\nand another line"), Screen::Prose);
        assert_eq!(
            classify("I will select the first option and confirm it later"),
            Screen::Prose
        );
    }

    /// Where the line actually falls, and what it costs. Coloured agent output counts as a control
    /// only when it offers BOTH a choice and the keys for it: one word alone, or colours without
    /// the words, leaves the text path open.
    ///
    /// The last case is the price this module accepts. A coloured line that happens to say both
    /// words reads as a control, and text replies to that session are refused until the screen
    /// redraws. That costs the operator one trip to their keyboard; the alternative costs them a
    /// keypress they never chose.
    #[test]
    fn coloured_output_is_a_control_only_when_it_offers_a_choice_and_the_keys() {
        let diff = "\u{1b}[32m+ let chosen = confirm(x);\u{1b}[0m  \u{1b}[31m- let chosen = old(x);\u{1b}[0m";
        assert_eq!(
            classify(diff),
            Screen::Prose,
            "a coloured diff must not cost the operator their reply path"
        );

        let status =
            "\u{1b}[1;34m main \u{1b}[0m\u{1b}[2m 3 files \u{1b}[0m\u{1b}[33m select \u{1b}[0m";
        assert_eq!(classify(status), Screen::Prose);

        // A word that stands alone as its own coloured run is read as part of a keybind footer,
        // which narrows this a good deal: highlighted SQL keywords do not trip it.
        assert_eq!(
            classify("\u{1b}[36mSELECT\u{1b}[0m \u{1b}[37mid FROM confirmations\u{1b}[0m"),
            Screen::Prose
        );

        // But glue either word to its neighbours and the row is a control. This is the price, and
        // it is real: the reply is refused until the screen redraws.
        assert_eq!(
            classify("\u{1b}[36mSELECT id\u{1b}[0m \u{1b}[37mFROM confirmations\u{1b}[0m"),
            Screen::UnreadableControl,
            "the accepted price: an ordinary coloured line can cost a reply"
        );
    }
}

//! Recognising a choice dialog in a pane, and driving it with the right keys.
//!
//! # The bug this exists to close
//!
//! An agent asking *"Access external directory ~/.local/share/example? — Allow once · Allow always
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
//! parser to one harness's theme, and break silently the day the operator changes it — silently,
//! because a dialog that stops being recognised falls back to being treated as prose, which is the
//! dangerous path. Instead: among the option runs, exactly one carries a background different from
//! the rest. That is true of any sane selector in any theme.

/// A choice dialog found in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The options, left to right, exactly as rendered.
    pub options: Vec<String>,
    /// Which one is highlighted right now.
    pub selected: usize,
}

impl Prompt {
    /// The keys that move the selection to `target` and confirm it.
    ///
    /// Computed from the selection observed in the *same read* that produced this prompt, so the
    /// arrow count is derived rather than assumed. Re-parse immediately before sending: nothing
    /// else drives this pane, but a stale index would confirm the wrong option, and that is not a
    /// mistake worth risking to save one RPC.
    ///
    /// Deliberately never wraps around the ends. Wrapping would be fewer keypresses in some cases,
    /// but it depends on the harness wrapping too — and if it does not, the selection stops at the
    /// edge and `Enter` confirms the wrong option.
    pub fn keys_to(&self, target: usize) -> Option<Vec<&'static str>> {
        if target >= self.options.len() {
            return None;
        }
        let mut keys = Vec::new();
        let step = if target > self.selected {
            "Right"
        } else {
            "Left"
        };
        for _ in 0..target.abs_diff(self.selected) {
            keys.push(step);
        }
        keys.push("Enter");
        Some(keys)
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

/// Parse a choice dialog out of an ANSI pane read, or `None` if the pane is not showing one.
///
/// Conservative by design. Anything it does not positively recognise falls through to the ordinary
/// text-reply path — so a false negative costs a worse reply experience, while a false positive
/// would put buttons on a pane where they do nothing. Neither is good, but the first is recoverable
/// by the operator and the second is confusing.
pub fn parse(ansi: &str) -> Option<Prompt> {
    let line = ansi.lines().find(is_option_row)?;
    let runs = sgr_runs(line);

    let mut options: Vec<(String, String)> = Vec::new();
    for (sgr, text) in runs {
        let t = text.trim();
        if t.is_empty() || t.chars().all(|c| "│┃╹▀ ".contains(c)) {
            continue;
        }
        if is_hint(t) {
            break; // the footer starts here
        }
        options.push((t.to_string(), background_of(&sgr)));
    }

    if options.len() < 2 {
        return None;
    }

    // The selected option is the one whose background differs from every other. Structural, so it
    // survives a theme change; and requiring EXACTLY one keeps an ambiguous render from being
    // guessed at.
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, bg) in &options {
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
    let selected = options.iter().position(|(_, bg)| bg == unique[0])?;

    Some(Prompt {
        options: options.into_iter().map(|(t, _)| t).collect(),
        selected,
    })
}

/// Is this the row carrying the options?
///
/// Requires the affordance footer — a row of words alone is prose, but a row that also tells the
/// user how to select and confirm is a control.
fn is_option_row(line: &&str) -> bool {
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
fn background_of(sgr: &str) -> String {
    // `48;2;r;g;b` (truecolor) or `48;5;n` (indexed) or `4Xm` (basic).
    for cap in sgr.split('\u{1b}') {
        if let Some(i) = cap.find("48;") {
            let rest = &cap[i..];
            let end = rest.find('m').unwrap_or(rest.len());
            return rest[..end].to_string();
        }
    }
    String::new()
}

fn strip_sgr(s: &str) -> String {
    sgr_runs(s).into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
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
    fn choosing_reject_sends_two_rights_and_enter() {
        let p = parse(REAL).unwrap();
        let reject = p.options.iter().position(|o| o == "Reject").unwrap();
        assert_eq!(p.keys_to(reject).unwrap(), ["Right", "Right", "Enter"]);
    }

    #[test]
    fn choosing_the_already_selected_option_just_confirms() {
        let p = parse(REAL).unwrap();
        assert_eq!(p.keys_to(0).unwrap(), ["Enter"]);
    }

    #[test]
    fn moving_left_is_supported_and_never_wraps() {
        let p = Prompt {
            options: vec!["a".into(), "b".into(), "c".into()],
            selected: 2,
        };
        assert_eq!(p.keys_to(0).unwrap(), ["Left", "Left", "Enter"]);
        assert!(
            p.keys_to(3).is_none(),
            "an out-of-range target must be refused, not wrapped"
        );
    }

    /// Every key this module emits must be one herdr accepts on protocol 20. `Left`, `Right` and
    /// `Enter` were all confirmed by the probe; `C-c`-style forms and named navigation keys such as
    /// `Home` were rejected (docs/SLICE-3-PROBE.md P2).
    #[test]
    fn every_emitted_key_is_one_the_probe_confirmed() {
        let p = parse(REAL).unwrap();
        for target in 0..p.options.len() {
            for k in p.keys_to(target).unwrap() {
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
}

//! ⚠ **UNVERIFIED-ON-P20.** `pane.send_keys`'s key grammar is `HERDR_API.md` evidence enumerated
//! against herdr 0.7.0–0.7.4 / **protocol 16**. It CANNOT be re-probed against a live herd: pane
//! lookup precedes key validation (a bogus pane returns `pane_not_found`, never `invalid_key`), so
//! validating it means typing into a real pane. The schema does not constrain keys at all —
//! `PaneSendKeysParams.keys` is `{"items":{"type":"string"},"type":"array"}`. `agent.send` was
//! **REMOVED** between p16 and p20, so a p16 fact is not a p20 fact. Settle it with
//! `scripts/verify-send-p20.sh` inside a throwaway `herdr --session probe`, and treat the per-agent
//! **submit** key as a per-harness table (only `claude` and `opencode` are live here).
//!
//! **This is a validating NEWTYPE, not a closed enum**, precisely so the p16 grammar is not encoded
//! as type-level truth. A closed enum would say "these are the keys", which is a claim this crate
//! cannot make on protocol 20; it would also make a key herdr *does* accept unrepresentable, which
//! is the failure mode that costs an operator a working send at 2 a.m.
//!
//! # What the p16 evidence says, recorded as EVIDENCE and not as a type
//!
//! For the record, so a later session inherits it rather than re-deriving it — none of this is
//! enforced here, and all of it is what `verify-send-p20.sh` exists to re-settle:
//!
//! - Special keys, bare and case-insensitive: `Up` `Down` `Left` `Right` `Tab` `Enter` `Escape`
//!   `Space` `Backspace` (alias `BS`), `F1`…`F12`.
//! - A one-character string is typed as that literal character (`"1"` answers a permission dialog;
//!   `["2","Enter"]` picks option 2 of a select).
//! - Modifier chords join with `+`: `ctrl+c`, `shift+tab`, `alt+Up`, `ctrl+alt+shift+p`. Modifiers
//!   are `ctrl` / `shift` / `alt` / `cmd` / `super`, case-insensitive, in any order.
//! - **NOT** supported at p16: tmux syntax (`C-c`, `BTab`) and `PageUp` `PageDown` `Home` `End`
//!   `Insert` `Delete` in any spelling. So **Ctrl-C is `ctrl+c`, never `C-c`** — the single most
//!   likely thing for a human to write by hand and have silently rejected.
//! - Multiple keys in one call are applied in order.
//!
//! # What IS enforced, and why only this much
//!
//! [`Key::parse`] rejects exactly three things, all of which are local facts rather than grammar
//! claims: an empty name, a whitespace-only name, and a name containing a raw `\n` or `\r`. The
//! last is the one that matters — `HERDR_API.md`'s 0.7.4 finding is that the send path writes RAW
//! bytes, so a newline smuggled inside a "key" would arrive at the PTY as a **real Enter**, i.e. it
//! would submit whatever is sitting in the operator's prompt. Everything else is left to the
//! server's own validator, which answers `invalid_key: unsupported key <X>` — loud, typed, and
//! recoverable.
//!
//! Other C0 control characters are deliberately **not** rejected: at p16 a one-character key is
//! typed as that literal character, so a blanket control-character ban would remove a capability
//! the evidence says exists, to guard a hazard the server already refuses. `\n` and `\r` are
//! special only because they are the two bytes that mean "submit" at a PTY.
//!
//! # Validate at CONFIG-LOAD, not at send time
//!
//! [`Key`] has **no `Deserialize` impl**, derived or otherwise. That is deliberate: a
//! `#[serde(transparent)]` `Deserialize` would let a config file construct a `Key` that never went
//! through [`Key::parse`], which is exactly the bypass this newtype exists to prevent. Slice 2's
//! config must read a `String` and call [`Key::parse`], so a bad key is a startup error the
//! operator sees on their own terminal — not a mystery at the moment they are trying to unblock an
//! agent from their phone.

use std::fmt;

use serde::Serialize;

/// One key name for `pane.send_keys` / `pane.send_input`.
///
/// A validating newtype over the wire string. Construct it with [`Key::parse`]; it serializes
/// transparently, so `&[Key]` is the JSON array of strings the schema declares.
///
/// See the module docs: the *grammar* is unverified on protocol 20 and this type makes no claim
/// about it. What it guarantees is narrower and local — no key it holds is empty, whitespace-only,
/// or carries a raw newline into the PTY.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Key(String);

impl Key {
    /// Validate one key name.
    ///
    /// Rejects empty, whitespace-only, and anything containing `\n` or `\r` — a newline inside a
    /// "key" would become a real Enter at the PTY, submitting whatever the operator has typed.
    /// Everything else is passed through for the server's own validator to accept or refuse with
    /// `invalid_key`.
    ///
    /// Call this at **config load**, not at send time.
    pub fn parse(s: &str) -> Result<Key, KeyParseError> {
        if s.is_empty() {
            return Err(KeyParseError::Empty);
        }
        // The newline check comes FIRST, before the whitespace one, and the order is load-bearing:
        // `"\n"` is whitespace-only, so the other order reports a bare newline as
        // `Whitespace { input: "\n" }` — a message that tells the operator to write `"Space"` when
        // what they actually have is a raw Enter about to be typed into a terminal. Caught by
        // `a_newline_can_never_be_smuggled_into_a_key`, which is why that test enumerates `"\n"`
        // and `"\r"` on their own rather than only inside longer strings.
        if let Some(offset) = s.find(['\n', '\r']) {
            return Err(KeyParseError::Newline {
                input: s.to_owned(),
                offset,
            });
        }
        if s.chars().all(char::is_whitespace) {
            return Err(KeyParseError::Whitespace {
                input: s.to_owned(),
            });
        }
        Ok(Key(s.to_owned()))
    }

    /// `"Enter"`.
    ///
    /// The one convenience constructor, and it is still p16 evidence rather than a p20 fact — see
    /// the module banner. It exists because "send the text, then submit it" is the shape every
    /// caller wants, and because the alternative is every call site spelling a string literal that
    /// nothing validates.
    ///
    /// The per-harness **submit** key is a separate open question: `Enter` is what the herdr API
    /// notes use, but whether a given TUI submits on it is a claude-vs-opencode fact, not a herdr
    /// fact.
    pub fn enter() -> Key {
        Key("Enter".to_owned())
    }

    /// The validated wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a key name was refused before it ever reached the wire.
///
/// Every variant carries the offending input, because this surfaces at config load and the
/// operator needs to know *which* line of their config is wrong.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyParseError {
    /// `""` — an empty key would be sent as an empty string and refused by the server, but it is
    /// almost always a config typo (a trailing comma, an unset variable) and is worth naming.
    #[error("a key name cannot be empty")]
    Empty,

    /// Whitespace only. `Space` is the key name for a space; `" "` is a mistake.
    #[error("key {input:?} is whitespace only (the space key is named \"Space\")")]
    Whitespace {
        /// The rejected input, verbatim.
        input: String,
    },

    /// A raw `\n` or `\r` inside the name.
    ///
    /// **The load-bearing one.** The send path writes raw bytes, so this would arrive at the PTY as
    /// a real Enter and submit whatever is sitting in the operator's prompt.
    #[error(
        "key {input:?} contains a raw newline at byte {offset}; at a PTY that is a real Enter, \
         not a key name"
    )]
    Newline {
        /// The rejected input, verbatim.
        input: String,
        /// Byte offset of the first `\n` or `\r`.
        offset: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newline_can_never_be_smuggled_into_a_key() {
        // THE test in this file. The send path writes RAW bytes, so a `\n` inside a "key" is a
        // real Enter at the PTY — it would submit whatever the operator has half-typed.
        for hostile in [
            "\n",
            "\r",
            "Enter\n",
            "\nEnter",
            "ctrl+c\r\n",
            "a\nb",
            "Down\nEnter",
        ] {
            let err = match Key::parse(hostile) {
                Ok(accepted) => panic!("{hostile:?} must be rejected, got {accepted:?}"),
                Err(e) => e,
            };
            assert!(
                matches!(err, KeyParseError::Newline { .. }),
                "{hostile:?} -> {err:?}"
            );
            assert!(
                err.to_string().contains("real Enter"),
                "the message must say what the hazard IS: {err}"
            );
        }

        let err = Key::parse("a\nb").unwrap_err();
        assert_eq!(
            err,
            KeyParseError::Newline {
                input: "a\nb".to_owned(),
                offset: 1
            }
        );
    }

    #[test]
    fn empty_and_whitespace_only_are_refused_with_distinct_variants() {
        assert_eq!(Key::parse("").unwrap_err(), KeyParseError::Empty);

        for blank in [" ", "  ", "\t", " \t "] {
            assert!(
                matches!(Key::parse(blank), Err(KeyParseError::Whitespace { .. })),
                "{blank:?} must be Whitespace, not Empty"
            );
        }

        // The message points at the fix rather than just refusing.
        assert!(
            Key::parse(" ")
                .unwrap_err()
                .to_string()
                .contains("\"Space\""),
            "{}",
            Key::parse(" ").unwrap_err()
        );
    }

    /// The whole point of the newtype: this crate does NOT decide the grammar. Every one of these
    /// is p16 evidence, several are p16-INVALID (`C-c`, `PageUp`), and all of them must construct
    /// — a closed enum would make the last two unrepresentable and would be *wrong* if p20 moved.
    #[test]
    fn the_grammar_is_not_encoded_as_type_level_truth() {
        for accepted in [
            "Enter",
            "enter",
            "Escape",
            "Space",
            "Backspace",
            "BS",
            "F1",
            "F12",
            "Up",
            "alt+Up",
            "ctrl+c",
            "shift+tab",
            "ctrl+alt+shift+p",
            "1",
            "a",
            ".",
            // p16 says these are `invalid_key`. They still PARSE: the server is the authority on
            // the grammar, and the client's job is to relay its refusal, not to pre-empt it.
            "C-c",
            "BTab",
            "PageUp",
            "Home",
            "Delete",
            // A protocol-21 key nobody has seen yet must also be constructible.
            "hyper+quantum",
        ] {
            let key = Key::parse(accepted).unwrap_or_else(|e| panic!("{accepted:?} -> {e}"));
            assert_eq!(key.as_str(), accepted);
            assert_eq!(key.to_string(), accepted);
        }
    }

    #[test]
    fn enter_is_the_documented_spelling_and_serializes_transparently() {
        assert_eq!(Key::enter().as_str(), "Enter");
        assert_eq!(Key::enter(), Key::parse("Enter").unwrap());

        // Transparent: a `&[Key]` is the array of plain strings `PaneSendKeysParams` declares.
        let keys = [Key::parse("2").unwrap(), Key::enter()];
        assert_eq!(
            serde_json::to_string(&keys).unwrap(),
            r#"["2","Enter"]"#,
            "the newtype must not add a wrapper object to the wire"
        );
    }

    /// A `Deserialize` impl would be a validation bypass — a config file could hand us a `Key`
    /// containing a newline without ever calling `parse`. Asserted here as a comment-with-teeth:
    /// if someone derives `Deserialize` on `Key`, this test still compiles, so the real guard is
    /// the module docs plus this note. What IS asserted is the round trip callers must use.
    #[test]
    fn config_load_goes_string_then_parse() {
        let from_config: Vec<String> =
            serde_json::from_str(r#"["ctrl+c","Enter"]"#).expect("a config list of key names");
        let keys: Result<Vec<Key>, KeyParseError> =
            from_config.iter().map(|s| Key::parse(s)).collect();
        let keys = keys.expect("both are valid");
        assert_eq!(keys, vec![Key::parse("ctrl+c").unwrap(), Key::enter()]);

        let hostile: Vec<String> = serde_json::from_str(r#"["Enter\n"]"#).unwrap();
        assert!(
            hostile
                .iter()
                .map(|s| Key::parse(s))
                .collect::<Result<Vec<_>, _>>()
                .is_err(),
            "validation must happen at config load, and it must FAIL there"
        );
    }
}

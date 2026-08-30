//! The append-only record of every keystroke this bridge put into a terminal.
//!
//! # Why append-only, and why local
//!
//! PLAN.md promises it, and Collie's posture is the precedent: a remote-control surface that types
//! into real terminals must leave a trail its owner can read afterwards. The trail answers the
//! question that matters after something goes wrong — *what did it actually send, where, and did it
//! land* — which no amount of scrollback can reconstruct once a pane has churned.
//!
//! It is deliberately **local-only**. The operator's replies are the most sensitive thing this
//! product handles, and D4 already accepts that they transit Telegram; there is no reason to write
//! them anywhere else that leaves the machine. The file is git-ignored (`*.audit.log`) and the
//! ignore rule landed in slice 1, before the file could exist.
//!
//! # Why the write happens before the outcome is known
//!
//! Each attempt is recorded in **two** records: `sent` before the write, `outcome` after. If the
//! bridge is killed between them — which is exactly when the operator most wants to know what
//! happened — the `sent` record still says what went into the pane. A single record written after
//! the fact would lose precisely the case it exists for.
//!
//! # Why nothing can dangle
//!
//! A `sent` record with no ending is not a trail, it is a question. So `sent` is written only once
//! every check has passed and the keys are about to go out, and every branch that deliberately
//! sends nothing writes [`Audit::refused`] instead. Every attempt therefore ends in `outcome`,
//! `failed` or `refused`, and a `sent` with no ending means the bridge died mid-write — which is
//! the one thing it is supposed to mean.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use herdr_client::PaneId;

use crate::deliver::Delivery;

/// An append-only audit log.
#[derive(Debug, Clone)]
pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/herdr-tg/keystrokes.audit.log`, else `~/.local/state/…`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("herdr-tg").join("keystrokes.audit.log")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record an intent to write, BEFORE the write happens.
    ///
    /// `chat` is recorded because "which of my chats sent this" is a question the operator will
    /// have if a bot is ever shared or a chat id is ever wrong.
    pub fn sent(&self, at: &str, chat: i64, pane: &PaneId, text: &str) -> std::io::Result<()> {
        let mut line = String::new();
        let _ = write!(
            line,
            "{at}\tsent\tchat={chat}\tpane={}\tbytes={}\ttext={}",
            pane.as_str(),
            text.len(),
            escape(text)
        );
        self.append(&line)
    }

    /// Record what was actually observed, AFTER the write.
    pub fn outcome(&self, at: &str, delivery: &Delivery) -> std::io::Result<()> {
        let mut line = String::new();
        let _ = write!(
            line,
            "{at}\toutcome\tpane={}\trung={:?}\tdetail={}",
            delivery.pane.as_str(),
            delivery.rung,
            escape(&delivery.detail)
        );
        self.append(&line)
    }

    /// Record a write that failed before any outcome could be observed.
    pub fn failed(&self, at: &str, pane: &PaneId, err: &str) -> std::io::Result<()> {
        let mut line = String::new();
        let _ = write!(
            line,
            "{at}\tfailed\tpane={}\terror={}",
            pane.as_str(),
            escape(err)
        );
        self.append(&line)
    }

    /// Record an attempt that was deliberately NOT made.
    ///
    /// The counterpart to `sent`: it is what keeps a `sent` record from standing alone when a check
    /// stopped the keys. `why` is written for the operator reading this file afterwards, so it says
    /// what was seen, not which branch was taken.
    pub fn refused(&self, at: &str, chat: i64, pane: &PaneId, why: &str) -> std::io::Result<()> {
        let mut line = String::new();
        let _ = write!(
            line,
            "{at}\trefused\tchat={chat}\tpane={}\twhy={}",
            pane.as_str(),
            escape(why)
        );
        self.append(&line)
    }

    /// One record, one line, opened append-only every time.
    ///
    /// `O_APPEND` rather than a held handle: the file is the durable artifact, and a long-lived
    /// handle would keep a deleted or rotated file alive while the operator stared at an empty one.
    /// Mode 0600 at creation — this holds the operator's own words.
    fn append(&self, line: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.path)?;
        writeln!(f, "{line}")
    }
}

/// Make a value safe to put in a tab-separated, one-record-per-line file.
///
/// A reply is operator-authored free text: it contains newlines (multi-line answers are this
/// product's default case) and can contain tabs. Either would silently split one record into two,
/// and a log whose record count can be manipulated by the thing it audits is not an audit log.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deliver::Rung;

    /// A path unique to THIS test.
    ///
    /// Tests in one binary run in parallel threads, so a path keyed only on the process id had
    /// every audit test writing to — and deleting — the same file. Three of them failed for that
    /// reason and not for any fault in the code being tested, which is exactly the kind of red that
    /// wastes an afternoon.
    fn tmp(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("herdr-tg-audit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        base.join("keystrokes.audit.log")
    }

    #[test]
    fn a_record_is_one_line_even_when_the_reply_is_multi_line() {
        let path = tmp("a_record_is_one_line_even_when_the_reply_is_multi_line");
        let audit = Audit::new(&path);
        audit
            .sent(
                "T",
                1,
                &PaneId::new("w1:p1"),
                "line one\nline two\twith a tab",
            )
            .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            body.lines().count(),
            1,
            "a multi-line reply split the record — the log's record count is then attacker-controlled"
        );
        assert!(body.contains("\\nline two\\twith a tab"));
    }

    #[test]
    fn records_append_and_never_truncate() {
        let path = tmp("records_append_and_never_truncate");
        let audit = Audit::new(&path);
        audit.sent("T1", 1, &PaneId::new("w1:p1"), "first").unwrap();
        audit
            .sent("T2", 1, &PaneId::new("w1:p1"), "second")
            .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().count(), 2);
        assert!(body.contains("first") && body.contains("second"));
    }

    /// The two-record shape is the point: if the bridge dies mid-write, `sent` still says what went
    /// into the pane.
    #[test]
    fn sent_and_outcome_are_separate_records() {
        let path = tmp("sent_and_outcome_are_separate_records");
        let audit = Audit::new(&path);
        let pane = PaneId::new("w1:p1");
        audit.sent("T1", 42, &pane, "ship it").unwrap();
        audit
            .outcome(
                "T2",
                &Delivery {
                    pane: pane.clone(),
                    rung: Rung::Echoed,
                    detail: "the text appeared in the pane".into(),
                },
            )
            .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\tsent\t") && lines[0].contains("chat=42"));
        assert!(lines[1].contains("\toutcome\t") && lines[1].contains("rung=Echoed"));
    }

    /// The other half of the two-record shape: an attempt that was stopped ends here, so a `sent`
    /// record with no ending can only mean the bridge died mid-write.
    #[test]
    fn an_attempt_that_was_stopped_leaves_a_record_of_its_own() {
        let path = tmp("an_attempt_that_was_stopped_leaves_a_record_of_its_own");
        let audit = Audit::new(&path);
        audit
            .refused(
                "T",
                42,
                &PaneId::new("w1:p1"),
                "that session moved on to a different question\nsecond line",
            )
            .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().count(), 1);
        let line = body.lines().next().unwrap();
        assert!(line.contains("\trefused\t"), "not a refusal record: {line}");
        assert!(line.contains("chat=42") && line.contains("pane=w1:p1"));
        assert!(
            line.contains("why=that session moved on to a different question\\nsecond line"),
            "the reason was not written, or not escaped: {line}"
        );
    }

    #[test]
    fn a_failed_write_is_recorded_too() {
        let path = tmp("a_failed_write_is_recorded_too");
        let audit = Audit::new(&path);
        audit
            .failed("T", &PaneId::new("w1:p1"), "herdr unreachable")
            .unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("\tfailed\t")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt as _;
        let path = tmp("perms");
        Audit::new(&path)
            .sent("T", 1, &PaneId::new("w1:p1"), "private")
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the audit log holds the operator's own words");
    }

    #[test]
    fn escaping_is_reversible_enough_to_be_unambiguous() {
        assert_eq!(escape("a\\b"), "a\\\\b");
        assert_eq!(escape("a\nb"), "a\\nb");
        // A literal backslash-n in the text must not be confused with a newline.
        assert_ne!(escape("a\\nb"), escape("a\nb"));
    }
}

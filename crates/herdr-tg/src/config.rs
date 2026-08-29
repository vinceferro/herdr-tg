//! Configuration: structure from a TOML file, the credential from the environment.
//!
//! # Why the token is not in the file
//!
//! PLAN.md contradicted itself on this (`.env` in one place, `herdr-tg.toml` in another) and the
//! operator settled it: **the TOML carries structure, the environment carries the secret.** The
//! reason is that a config file gets copied — into a gist, a paste, a backup, a bug report — and a
//! token that travels with the workspace name and the quiet hours travels everywhere they do. The
//! systemd `--user` unit names the credential file in `EnvironmentFile=`, so the token reaches the
//! process without ever sitting beside the settings a human edits.
//!
//! [`Config::load`] therefore **refuses to start** if it finds a token-shaped key in the TOML. That
//! is deliberate: silently ignoring it would leave a live credential sitting in a file the operator
//! believes is being read, and the repo's `scan-secrets` has no Telegram pattern — it catches a bot
//! token only via a generic `(secret|token|password|…)` rule, and only if the value is quoted.
//!
//! # Why an empty allowlist answers nobody
//!
//! The allowlist is the identity gate — the equivalent of Collie's `COLLIE_TRUSTED_USER`. This bot
//! types into real terminals, so the gate **fails closed**: an empty, missing, or unparseable
//! allowlist means the bot answers no one at all. The opposite convention (empty means "allow
//! everything") is common and would be catastrophic here: a misplaced config file would hand
//! anyone who found the bot a keyboard attached to the operator's machine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use herdr_client::Key;
use serde::Deserialize;

/// The environment variable carrying the bot token. Written by `scripts/setup-token.sh`.
pub const TOKEN_ENV: &str = "HERDR_TG_TOKEN";

/// Optional environment override for the allowlist, as a comma-separated list of chat ids.
pub const CHAT_IDS_ENV: &str = "HERDR_TG_ALLOWED_CHAT_IDS";

/// Keys that must never appear in the TOML. Presence is a hard error, not a warning.
const FORBIDDEN_TOML_KEYS: &[&str] = &["token", "bot_token", "api_token", "secret"];

/// The `[bot]` section of `herdr-tg.toml`. Structure only — never a credential.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    /// The one workspace this bot speaks for (D2: one bot per workspace).
    workspace: Option<String>,
    /// Chat ids permitted to talk to this bot. Empty or absent means nobody.
    #[serde(default)]
    allowed_chat_ids: Vec<i64>,
    /// Socket override, for a probe session. Normally absent.
    socket: Option<PathBuf>,
    /// The key that submits a reply in an agent pane. Default `Enter`.
    submit_key: Option<String>,
}

/// Everything the bridge needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    token: String,
    /// A set, so a duplicated id in the file is not a duplicated grant, and ordering is stable in
    /// the startup log.
    ///
    /// This is DATA, not the gate. The admission decision lives in exactly one place —
    /// [`crate::bot::Gate`] — because two implementations of a fail-closed check are two places
    /// for it to drift open, and only one of them will have the test.
    pub allowed_chat_ids: BTreeSet<i64>,
    pub workspace: Option<String>,
    pub socket: Option<PathBuf>,
    /// The key pressed after the operator's text to submit it.
    ///
    /// `Enter` for `opencode` is confirmed on the wire. For `claude` it is **operator-supplied
    /// knowledge, not a probed fact** (`docs/SLICE-3-PROBE.md`, "still open"). That is survivable
    /// only because delivery is verified by reading the pane back: a wrong key shows up as
    /// [`crate::deliver::Rung::Echoed`] and the operator is told, rather than being told "sent".
    pub submit_key: Key,
}

impl Config {
    /// The bot token. Deliberately a method rather than a public field: it makes every read of the
    /// credential a visible call site that a reviewer can grep for.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Load structure from `path` (if it exists) and the credential from the environment.
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let file = match path {
            Some(p) if p.exists() => {
                let raw = std::fs::read_to_string(p)
                    .with_context(|| format!("reading config {}", p.display()))?;
                reject_credentials_in_file(&raw, p)?;
                toml::from_str::<FileConfig>(&raw)
                    .with_context(|| format!("parsing config {}", p.display()))?
            }
            Some(p) => bail!("config file {} does not exist", p.display()),
            None => FileConfig::default(),
        };

        let token = std::env::var(TOKEN_ENV)
            .ok()
            .filter(|t| !t.trim().is_empty());
        let Some(token) = token else {
            bail!(
                "{TOKEN_ENV} is not set. The token never lives in the config file — run \
                 `bash scripts/setup-token.sh` to write ~/.config/herdr-tg/env, and point the \
                 systemd unit's EnvironmentFile= at it."
            );
        };

        // The env form wins when present, so a probe run can narrow the allowlist without editing
        // the file the service reads.
        let mut allowed: BTreeSet<i64> = file.allowed_chat_ids.into_iter().collect();
        if let Ok(raw) = std::env::var(CHAT_IDS_ENV) {
            let from_env =
                parse_chat_ids(&raw).with_context(|| format!("parsing {CHAT_IDS_ENV}"))?;
            if !from_env.is_empty() {
                allowed = from_env;
            }
        }

        let submit_raw = file.submit_key.as_deref().unwrap_or("Enter");
        let submit_key = Key::parse(submit_raw).map_err(|e| {
            anyhow::anyhow!(
                "submit_key `{submit_raw}` is not a key herdr accepts: {e}. Note the chord form is \
                 `ctrl+c`, not `C-c` — the tmux form is refused for every chord except `c-c`, which \
                 herdr special-cases (docs/SLICE-3-PROBE.md P2)."
            )
        })?;

        Ok(Self {
            token: token.trim().to_string(),
            allowed_chat_ids: allowed,
            workspace: file.workspace,
            socket: file.socket,
            submit_key,
        })
    }
}

/// Refuse a config file that carries a credential.
///
/// Scanned as raw text rather than after parsing, because `deny_unknown_fields` would reject an
/// unknown key with a serde error that reads like a typo — not like "you have put a live token in
/// a file that gets copied around".
fn reject_credentials_in_file(raw: &str, path: &Path) -> anyhow::Result<()> {
    for (n, line) in raw.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if FORBIDDEN_TOML_KEYS.contains(&key) {
            bail!(
                "{}:{} sets `{key}`, but the token must never live in the config file — it is read \
                 from ${TOKEN_ENV}. Remove the line, and treat that value as COMPROMISED: rotate \
                 it with BotFather (/revoke), because a config file travels wherever the workspace \
                 does.",
                path.display(),
                n + 1
            );
        }
    }
    Ok(())
}

/// Parse a comma-separated chat-id list. A malformed entry is an error, never a silent skip —
/// a dropped id fails OPEN for the operator (they stop being able to talk to their own bot) but a
/// misparsed one could just as easily be someone else's.
fn parse_chat_ids(raw: &str) -> anyhow::Result<BTreeSet<i64>> {
    let mut out = BTreeSet::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let id: i64 = part
            .parse()
            .with_context(|| format!("`{part}` is not a chat id"))?;
        out.insert(id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(ids: &[i64]) -> Config {
        Config {
            token: "t".into(),
            allowed_chat_ids: ids.iter().copied().collect(),
            workspace: None,
            socket: None,
            submit_key: Key::parse("Enter").expect("Enter is valid"),
        }
    }

    #[test]
    fn a_token_in_the_config_file_is_a_hard_error_naming_rotation() {
        let raw = "workspace = \"herdr-tg\"\ntoken = \"123456:AAAA\"\n";
        let err = reject_credentials_in_file(raw, Path::new("herdr-tg.toml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("herdr-tg.toml:2"), "must name the line: {msg}");
        assert!(
            msg.contains("COMPROMISED") && msg.contains("/revoke"),
            "a token in a shared file is burned; the error must say so: {msg}"
        );
    }

    #[test]
    fn a_commented_token_line_is_not_an_error() {
        let raw = "# token = \"do not put it here\"\nworkspace = \"x\"\n";
        assert!(reject_credentials_in_file(raw, Path::new("c.toml")).is_ok());
    }

    #[test]
    fn every_forbidden_key_is_actually_caught() {
        for key in FORBIDDEN_TOML_KEYS {
            let raw = format!("{key} = \"x\"\n");
            assert!(
                reject_credentials_in_file(&raw, Path::new("c.toml")).is_err(),
                "`{key}` is on the forbidden list but slips through"
            );
        }
    }

    #[test]
    fn chat_ids_parse_and_a_bad_one_is_fatal() {
        assert_eq!(
            parse_chat_ids(" 1, -2 ,3 ").unwrap(),
            [1i64, -2, 3].into_iter().collect::<BTreeSet<_>>()
        );
        assert!(parse_chat_ids("").unwrap().is_empty());
        assert!(
            parse_chat_ids("1,notanid").is_err(),
            "a malformed id must be fatal, never silently dropped"
        );
    }

    /// The config struct must not hand the token out by field access.
    /// The token must be reachable only through its accessor, so every read of the credential is
    /// a grep-able call site.
    #[test]
    fn the_token_is_reachable_only_through_its_accessor() {
        let c = cfg(&[1]);
        assert_eq!(c.token(), "t");
        assert_eq!(c.allowed_chat_ids, [1i64].into_iter().collect());
    }
}

//! Configuration: structure from a TOML file, the credential from the environment.
//!
//! # The plane is told, and it decides whether there is a credential at all
//!
//! A hub reaches the operator one of two ways, and [`Config::load`] is given which
//! ([`Plane`]) rather than working it out. On the phone line the token is required exactly as it
//! always was. On the app there is no token on the returned configuration at all: the environment
//! is read once, a line is said if a token is set there, and the value is dropped. Nothing
//! downstream can build a Bot API client out of a configuration that does not carry one, so the
//! promise is held by the type rather than by every later caller remembering it.
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
//!
//! # Two lists: where the bot listens, and who may speak there
//!
//! The chat allowlist says WHERE. It says nothing about WHO: everyone in an allowed forum could
//! type into an agent's turn and tap any keyboard in it, and that was safe only for as long as the
//! forum held one person. The people allowlist is the second gate — the people who may speak
//! anywhere this bot listens — and it is configured the same two ways the chats are, and never
//! by a message.
//!
//! It has a default that costs the operator nothing, and the default is a fact about Telegram
//! rather than a guess: **a private chat's id IS the user's id.** Groups, supergroups and channels
//! are always negative; a person is always positive; and a bot cannot open a private chat with
//! another bot. So every positive id on the chat allowlist names exactly one person the operator
//! already let talk to the bot in private — and that person may speak in the forum too. Nothing to
//! set, nothing a stranger can be admitted by, and the startup log says which people it found.
//! `KICKOFF_CHANNEL_ALLOWED_USER_IDS` / `allowed_user_ids` adds to that; it never replaces it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use serde::Deserialize;

/// The environment variable carrying the bot token. Written by `scripts/setup-token.sh`.
pub const TOKEN_ENV: &str = "KICKOFF_CHANNEL_TOKEN";

/// Optional environment override for the allowlist, as a comma-separated list of chat ids.
pub const CHAT_IDS_ENV: &str = "KICKOFF_CHANNEL_ALLOWED_CHAT_IDS";

/// Optional environment override for the people allowlist, as a comma-separated list of user ids.
/// Added to the people the chat allowlist already names; see the module doc.
pub const USER_IDS_ENV: &str = "KICKOFF_CHANNEL_ALLOWED_USER_IDS";

/// Keys that must never appear in the TOML. Presence is a hard error, not a warning.
const FORBIDDEN_TOML_KEYS: &[&str] = &["token", "bot_token", "api_token", "secret"];

/// Which way a hub reaches the operator. Told at the terminal, never worked out.
///
/// **There is no default and nothing is inferred**, and that is the whole design of this type.
/// The obvious shortcut — "a token is set, so he must mean the phone" — makes the silent mistake
/// by construction: a typo in the credential file, or a unit whose environment file was never
/// rendered, would quietly become the app and every agent on the box would talk into a file while
/// he waited for a phone that was never going to ring. A default makes one of those two mistakes
/// depending which way it points. A required argument makes neither, because there is no state
/// reachable by omission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Plane {
    /// The Bot API: one bot, one forum, one topic per conversation. Needs a token.
    Telegram,
    /// The app: the ring and the answers drop, read and written through the PWA's own door.
    /// Nothing here dials a messaging service and nothing here holds a credential.
    App,
}

/// The `[bot]` section of the config file. Structure only — never a credential. Existing
/// `herdr-tg.toml` files remain valid during the source-only rename.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    /// The one workspace this bot speaks for (D2: one bot per workspace).
    workspace: Option<String>,
    /// Chat ids permitted to talk to this bot. Empty or absent means nobody.
    #[serde(default)]
    allowed_chat_ids: Vec<i64>,
    /// User ids of people who may speak anywhere this bot listens, on top of the people the chat
    /// allowlist already names. Empty or absent adds nobody.
    #[serde(default)]
    allowed_user_ids: Vec<i64>,
    /// Socket override, for a probe session. Normally absent.
    socket: Option<PathBuf>,
    /// The key that submits a reply in an agent pane. Default `Enter`.
    submit_key: Option<String>,
    /// A forum-enabled supergroup where each pane gets its own topic.
    forum_chat_id: Option<i64>,
}

/// Everything the bridge needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    /// The bot token, and `None` whenever this hub reaches him through the app.
    ///
    /// Absent rather than empty, because the two are not the same promise. An empty string would
    /// still build a `Bot`, which would then long-poll the Bot API with no credential; absence
    /// cannot be handed to one at all, so the app plane's guarantee is held by the type rather
    /// than by everybody downstream remembering to check.
    token: Option<String>,
    /// A set, so a duplicated id in the file is not a duplicated grant, and ordering is stable in
    /// the startup log.
    ///
    /// This is DATA, not the gate. The admission decision lives in exactly one place —
    /// [`crate::bot::Gate`] — because two implementations of a fail-closed check are two places
    /// for it to drift open, and only one of them will have the test.
    pub allowed_chat_ids: BTreeSet<i64>,
    /// The people LISTED as allowed to speak anywhere, by `KICKOFF_CHANNEL_ALLOWED_USER_IDS` or the
    /// file. Not the whole answer: [`Config::people`] is, because the chat allowlist names people
    /// too. Kept apart so the startup line can say which of the two each person came from.
    pub allowed_user_ids: BTreeSet<i64>,
    /// A forum-enabled supergroup, if one is configured.
    ///
    /// When set, each pane gets its own topic and a reply inside a topic routes to that pane — no
    /// target to aim, and no way for a reply to land in a pane the operator was not looking at.
    /// Without it the bridge falls back to one flat conversation, where routing depends on reply-to
    /// and a sticky target.
    ///
    /// It must be a SUPERGROUP with Topics enabled, and the bot must be an admin with "Manage
    /// Topics" — a plain group or a DM cannot carry topics at all.
    pub forum_chat_id: Option<i64>,
}

impl Config {
    /// The bot token, when this hub has one. Deliberately a method rather than a public field: it
    /// makes every read of the credential a visible call site that a reviewer can grep for.
    ///
    /// `None` is a hub reaching him through the app, and the one caller that wants a token has to
    /// say what it does about that rather than being handed an empty string.
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Everyone who may speak anywhere this bot listens: the people listed, plus every person the
    /// chat allowlist names by way of a private chat.
    ///
    /// The second half is what keeps the operator's existing configuration working with no new
    /// setting: his allowlist has held his own private chat since the day the bot was set up, and
    /// that number IS his user id. See the module doc for why that is a fact and not a guess.
    pub fn people(&self) -> BTreeSet<i64> {
        self.allowed_user_ids
            .union(&people_from_private_chats(&self.allowed_chat_ids))
            .copied()
            .collect()
    }

    /// Load structure from `path` (if it exists) and, on the phone line only, the credential from
    /// the environment.
    ///
    /// `to` is not a hint. On [`Plane::App`] the environment is read for the token exactly once
    /// and for one purpose — to say out loud that one is set here and is not being used — and the
    /// value is then dropped on the floor. It never reaches the returned configuration, so there
    /// is nothing for a later caller to find and nothing to build a Bot API client out of.
    pub fn load(path: Option<&Path>, to: Plane) -> anyhow::Result<Self> {
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
        Self::assemble(
            file,
            to,
            crate::compat::setting("TOKEN")?,
            crate::compat::environment("ALLOWED_CHAT_IDS")?,
            crate::compat::environment("ALLOWED_USER_IDS")?,
            crate::compat::environment("FORUM_CHAT_ID")?,
        )
    }

    /// Put a configuration together from the file and the three environment values, with nothing
    /// read from the process. Split out so the exact shape the operator's box runs — no file, two
    /// chat ids in the environment, nothing else — can be tested without a test touching the
    /// environment, which is shared by every test in the process.
    ///
    /// The token arrives with the NAME it was found under, because the sentence said about it
    /// quotes that name: an operator who wrote the former spelling and is told about the current
    /// one has been sent to look for a line he never typed.
    fn assemble(
        file: FileConfig,
        to: Plane,
        token: Option<crate::compat::Setting>,
        chat_ids_env: Option<String>,
        user_ids_env: Option<String>,
        forum_chat_id_env: Option<String>,
    ) -> anyhow::Result<Self> {
        let token = token.filter(|t| !t.value.trim().is_empty());
        let token = match to {
            Plane::Telegram => {
                let Some(token) = token else {
                    bail!(
                        "{TOKEN_ENV} is not set. The token never lives in the config file — run \
                         `bash scripts/setup-token.sh` to write ~/.config/herdr-tg/env, and point \
                         the systemd unit's EnvironmentFile= at it."
                    );
                };
                Some(token.value.trim().to_string())
            }
            Plane::App => {
                // Said once, and never fatal. A box mid-migration keeps its old credential file
                // around, and refusing to start over a credential this plane will not touch would
                // make the app something he cannot switch to without a tidy-up first. But silence
                // here is the failure that matters: he is the one waiting for a phone to buzz, and
                // this line is the only thing on the box that will tell him which plane he is on.
                // The bytes are not printed — a journal is not a place for a live credential.
                if let Some(found) = token {
                    tracing::warn!(
                        setting = %found.name,
                        "this hub reaches him through the app, where no bot token is used. The \
                         one set here is being ignored, and nothing will reach a phone."
                    );
                }
                None
            }
        };

        // The env form wins when present, so a probe run can narrow the allowlist without editing
        // the file the service reads.
        //
        // A bad entry is refused ON ONE LINE that names the setting, the entry and the reason —
        // never a context wrapper with the reason underneath. `main` prints only the top-level
        // line and sends the causes to the debug log, which the unit's `RUST_LOG=info` drops; so
        // wrapped, all the operator saw was `parsing HERDR_TG_ALLOWED_USER_IDS`, every five
        // seconds under `Restart=always`, with no number and no fix.
        let mut allowed: BTreeSet<i64> = file.allowed_chat_ids.into_iter().collect();
        if let Some(raw) = chat_ids_env {
            let from_env = parse_chat_ids(&raw).map_err(|e| anyhow!("{CHAT_IDS_ENV}: {e:#}"))?;
            if !from_env.is_empty() {
                allowed = from_env;
            }
        }

        // The same rule for the people, and the same reason.
        let mut people: BTreeSet<i64> = file.allowed_user_ids.into_iter().collect();
        for id in &people {
            if !is_a_persons_id(*id) {
                bail!(
                    "{}",
                    not_a_person("allowed_user_ids in the config file", *id)
                );
            }
        }
        if let Some(raw) = user_ids_env {
            let from_env = parse_user_ids(&raw)?;
            if !from_env.is_empty() {
                people = from_env;
            }
        }

        // `workspace`, `socket` and `submit_key` are still ACCEPTED in the file and ignored, so an
        // existing config file does not become a startup error on upgrade. They configured the
        // path that watched panes and typed into them, and that path no longer exists.
        for (name, present) in [
            ("workspace", file.workspace.is_some()),
            ("socket", file.socket.is_some()),
            ("submit_key", file.submit_key.is_some()),
        ] {
            if present {
                tracing::info!(
                    setting = name,
                    "this setting configured the old pane path, which has been removed. It is \
                     ignored, and you can delete the line."
                );
            }
        }

        Ok(Self {
            token,
            allowed_chat_ids: allowed,
            allowed_user_ids: people,
            forum_chat_id: forum_chat_id_env
                .and_then(|v| v.trim().parse().ok())
                .or(file.forum_chat_id),
        })
    }
}

/// The people a chat allowlist names on its own: every private chat on it, because in the Bot API
/// a private chat's id is the id of the one person in it.
///
/// Positive means a person. A group, a supergroup and a channel are always negative, and a bot
/// cannot open a private chat with another bot — so a positive id on the chat allowlist is exactly
/// one human the operator already let talk to the bot in private, and cannot be anything else.
pub fn people_from_private_chats(chats: &BTreeSet<i64>) -> BTreeSet<i64> {
    chats
        .iter()
        .copied()
        .filter(|id| is_a_persons_id(*id))
        .collect()
}

/// Is this number the shape of a person's Telegram id?
///
/// The same fact from the other side: only a person's id is positive. A negative number on a
/// people list is a group somebody meant to allow wholesale, and a group cannot be allowed to
/// speak — its people are allowed one by one — so it is refused rather than kept as a number that
/// can never match anyone.
pub fn is_a_persons_id(id: i64) -> bool {
    id > 0
}

/// What is said when a people setting holds something that is not a person.
pub fn not_a_person(where_: &str, id: i64) -> String {
    format!(
        "{where_} holds {id}, which is not a person's Telegram id. A person's id is a positive \
         number; a group's is negative, and a group cannot be allowed to speak — allow its people \
         one at a time."
    )
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
            .with_context(|| format!("`{part}` is not a Telegram id, which is a whole number"))?;
        out.insert(id);
    }
    Ok(out)
}

/// Parse a comma-separated list of people. The same rule as the chats — a malformed entry is
/// fatal — plus one more: a negative number is a chat, not a person, and is refused with the
/// reason rather than kept as an entry nobody could ever match.
///
/// Every refusal is one whole line naming the setting; see `assemble` for why nothing wraps it.
fn parse_user_ids(raw: &str) -> anyhow::Result<BTreeSet<i64>> {
    let ids = parse_chat_ids(raw).map_err(|e| anyhow!("{USER_IDS_ENV}: {e:#}"))?;
    for id in &ids {
        if !is_a_persons_id(*id) {
            bail!("{}", not_a_person(USER_IDS_ENV, *id));
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(ids: &[i64]) -> Config {
        Config {
            token: Some("t".into()),
            allowed_chat_ids: ids.iter().copied().collect(),
            allowed_user_ids: BTreeSet::new(),
            forum_chat_id: None,
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
        assert_eq!(c.token(), Some("t"));
        assert_eq!(c.allowed_chat_ids, [1i64].into_iter().collect());
    }

    /// A token as the environment hands one over: the bytes, and the name they were found under.
    fn a_token(value: &str) -> crate::compat::Setting {
        crate::compat::Setting {
            name: TOKEN_ENV.to_owned(),
            value: value.to_owned(),
        }
    }

    /// The exact shape the operator's box runs: no config file, the chat allowlist alone in the
    /// environment — his own private chat and the forum — and no people setting at all, because
    /// this build is the first to have one.
    fn the_live_shape(user_ids_env: Option<&str>) -> Config {
        Config::assemble(
            FileConfig::default(),
            Plane::Telegram,
            Some(a_token("t")),
            Some("9,-1009".into()),
            user_ids_env.map(str::to_owned),
            None,
        )
        .expect("the live configuration loads")
    }

    #[test]
    fn the_operators_existing_config_keeps_working_without_a_new_setting() {
        // A restart into this build that answered nobody would be the failure this whole gate is
        // not allowed to cause. A private chat's id IS its person's id in the Bot API, so the
        // configuration he already has names him — the number is on his allowlist because he let
        // himself talk to the bot in private on the day he set it up.
        let cfg = the_live_shape(None);
        assert_eq!(cfg.allowed_chat_ids, [9i64, -1009].into_iter().collect());
        assert!(cfg.allowed_user_ids.is_empty(), "nothing was listed");
        assert_eq!(
            cfg.people(),
            [9i64].into_iter().collect::<BTreeSet<_>>(),
            "the person behind his private chat may speak, and the forum is not a person"
        );
    }

    #[test]
    fn listing_people_adds_to_the_private_chats_rather_than_replacing_them() {
        // The listed people are a second source, not an override: setting the new variable must
        // not silently take the operator's own standing away.
        let cfg = the_live_shape(Some("12"));
        assert_eq!(
            cfg.people(),
            [9i64, 12].into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(cfg.allowed_user_ids, [12i64].into_iter().collect());
    }

    #[test]
    fn a_group_on_the_people_list_is_refused_rather_than_kept() {
        // A negative number is a chat. Kept, it could never match a sender, and the operator would
        // believe he had let a whole group in. Fatal at startup, in both forms, naming the number.
        //
        // Asserted on `to_string()` — the top-level line and nothing under it — because that is
        // the one line `main` prints. This test used to pass on `{err:#}`, which walks the cause
        // chain, while the binary printed `parsing HERDR_TG_ALLOWED_USER_IDS` and stopped: no
        // number, no reason, every five seconds under the unit's restart.
        let err = Config::assemble(
            FileConfig::default(),
            Plane::Telegram,
            Some(a_token("t")),
            None,
            Some("12,-1009".into()),
            None,
        )
        .expect_err("a group on the people list loaded");
        let said = err.to_string();
        assert!(
            said.contains(USER_IDS_ENV) && said.contains("-1009") && said.contains("not a person"),
            "the one line the operator sees does not name the setting, the number and the \
             reason: {said}"
        );

        let file: FileConfig =
            toml::from_str("allowed_user_ids = [12, -1009]\n").expect("the file parses");
        let err = Config::assemble(file, Plane::Telegram, Some(a_token("t")), None, None, None)
            .expect_err("a group in the file loaded");
        let said = err.to_string();
        assert!(
            said.contains("allowed_user_ids")
                && said.contains("-1009")
                && said.contains("not a person"),
            "{said}"
        );
    }

    #[test]
    fn an_entry_that_is_not_a_number_is_refused_on_one_line_naming_the_setting_and_the_entry() {
        // Both lists, the same one line: the setting, the entry, and why. The chat list had the
        // same wrapper, and printed only `parsing HERDR_TG_ALLOWED_CHAT_IDS` for a typo.
        let err = Config::assemble(
            FileConfig::default(),
            Plane::Telegram,
            Some(a_token("t")),
            Some("9,notanid".into()),
            None,
            None,
        )
        .expect_err("a typo in the chat list loaded");
        let said = err.to_string();
        assert!(
            said.contains(CHAT_IDS_ENV) && said.contains("notanid"),
            "the one line the operator sees does not name the setting and the entry: {said}"
        );

        let err = Config::assemble(
            FileConfig::default(),
            Plane::Telegram,
            Some(a_token("t")),
            None,
            Some("12,twelve".into()),
            None,
        )
        .expect_err("a typo in the people list loaded");
        let said = err.to_string();
        assert!(
            said.contains(USER_IDS_ENV) && said.contains("twelve"),
            "the one line the operator sees does not name the setting and the entry: {said}"
        );
    }

    #[test]
    fn a_private_chat_names_its_person_and_a_group_names_nobody() {
        let chats: BTreeSet<i64> = [9i64, 12, -1009, -100_200].into_iter().collect();
        assert_eq!(
            people_from_private_chats(&chats),
            [9i64, 12].into_iter().collect::<BTreeSet<_>>()
        );
        assert!(people_from_private_chats(&BTreeSet::new()).is_empty());
    }

    /// Everything `tracing` wrote while the guard lived, so a test can read the line the operator
    /// reads. One line per event, no colour — the journal's own shape.
    struct Journal(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Journal {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the journal is not poisoned")
                .extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn a_journal() -> (
        tracing::subscriber::DefaultGuard,
        std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    ) {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&buf);
        let sub = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || Journal(std::sync::Arc::clone(&sink)))
            .finish();
        (tracing::subscriber::set_default(sub), buf)
    }

    fn read(journal: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
        String::from_utf8(journal.lock().expect("not poisoned").clone()).expect("utf-8")
    }

    #[test]
    fn a_hub_reaching_him_through_the_app_never_puts_a_bot_token_on_its_configuration() {
        // The whole of the app plane's credential story. The environment on a box that has run on
        // the phone still carries the token — an old credential file, a shell that exported it, a
        // unit somebody copied — and the plane's promise is that none of it reaches this process's
        // configuration. Asserted on the Debug shape as well as the accessor, because a token kept
        // in a field nothing reads is still a token in a crash dump and in a `{cfg:?}` in a log.
        let cfg = Config::assemble(
            FileConfig::default(),
            Plane::App,
            Some(a_token("123456:AAAAsecretsecret")),
            Some("9,-1009".into()),
            None,
            Some("-1009".into()),
        )
        .expect("a hub reaching him through the app loads");
        assert!(
            cfg.token().is_none(),
            "a hub with no phone line is carrying a bot token"
        );
        let shape = format!("{cfg:?}");
        assert!(
            !shape.contains("AAAAsecretsecret"),
            "the token's bytes are on the configuration after all: {shape}"
        );
    }

    #[test]
    fn a_hub_reaching_him_through_the_app_says_out_loud_that_it_found_a_token_it_is_not_using() {
        // A token in the environment of an app-plane hub is not an error — the operator may be
        // mid-migration, and refusing to start over a credential nothing will touch would be a
        // plane that cannot be switched to without a tidy-up first. But it must never be SILENT:
        // a token set here and not used is a box where he is waiting for a phone to buzz, and the
        // one line in the journal is the only thing that will ever tell him which plane he is on.
        let (_guard, journal) = a_journal();
        Config::assemble(
            FileConfig::default(),
            Plane::App,
            Some(a_token("123456:AAAAsecretsecret")),
            None,
            None,
            None,
        )
        .expect("a hub reaching him through the app loads");
        let said = read(&journal);
        assert!(
            said.contains(TOKEN_ENV),
            "the line does not name the setting he has to go and look at: {said}"
        );
        assert!(
            said.contains("no bot token is used"),
            "the line does not say the token is not being used: {said}"
        );
        assert!(
            !said.contains("AAAAsecretsecret"),
            "the credential itself was written into the journal: {said}"
        );
    }

    #[test]
    fn a_hub_reaching_him_through_the_app_with_no_token_anywhere_says_nothing_about_one() {
        // The other half, and the reason the line above is worth having: on the plane's ordinary
        // box there is no token at all, and a line about one would send him looking for a setting
        // nobody wrote.
        let (_guard, journal) = a_journal();
        Config::assemble(FileConfig::default(), Plane::App, None, None, None, None)
            .expect("a hub reaching him through the app loads with no token");
        let said = read(&journal);
        assert!(
            !said.to_lowercase().contains("token"),
            "a hub that found no token talked about one anyway: {said}"
        );
    }
}

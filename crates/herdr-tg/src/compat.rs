//! One-way compatibility for the product rename.
//!
//! Fresh configuration uses `KICKOFF_CHANNEL_*`. Existing installs may keep `HERDR_TG_*` while
//! they migrate. If both spellings are present they must carry the same bytes: silently choosing
//! one would make the credential, allowlist or state directory that an operator inspected differ
//! from the one the process actually used.

use std::ffi::OsString;

use anyhow::{Context, bail};

pub const CANONICAL_PREFIX: &str = "KICKOFF_CHANNEL_";
pub const LEGACY_PREFIX: &str = "HERDR_TG_";

/// One setting, and the name it was actually found under.
///
/// The name travels with the value because the operator-facing refusals quote it. Someone who set
/// `KICKOFF_CHANNEL_SUMMARIZER_URL` and had it refused must be told about the variable they set,
/// not about the other spelling of it — a sentence naming a variable they never wrote reads as a
/// bug in the program rather than a thing they can go and fix.
#[derive(Debug)]
pub struct Setting {
    /// The environment variable this came from, in the spelling the operator used.
    pub name: String,
    pub value: String,
}

/// Read one renamed setting, accepting the old spelling without giving it precedence.
pub fn environment(suffix: &str) -> anyhow::Result<Option<String>> {
    Ok(setting(suffix)?.map(|found| found.value))
}

/// As [`environment`], keeping the name the value was found under.
pub fn setting(suffix: &str) -> anyhow::Result<Option<Setting>> {
    let canonical_name = format!("{CANONICAL_PREFIX}{suffix}");
    let legacy_name = format!("{LEGACY_PREFIX}{suffix}");
    resolve(
        &canonical_name,
        std::env::var_os(&canonical_name),
        &legacy_name,
        std::env::var_os(&legacy_name),
    )
}

fn resolve(
    canonical_name: &str,
    canonical: Option<OsString>,
    legacy_name: &str,
    legacy: Option<OsString>,
) -> anyhow::Result<Option<Setting>> {
    let selected = match (canonical, legacy) {
        (Some(current), Some(old)) if current != old => {
            bail!(
                "{canonical_name} and its former name {legacy_name} are both set differently. \
                 Kickoff Channel will not guess which one controls this run; keep one, or make \
                 them identical."
            )
        }
        (Some(current), _) => Some((canonical_name, current)),
        (None, Some(old)) => Some((legacy_name, old)),
        (None, None) => None,
    };

    selected
        .map(|(name, value)| {
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("{name} is not valid text, so it cannot be used"))
                .with_context(|| format!("reading {name}"))
                .map(|value| Setting {
                    name: name.to_owned(),
                    value,
                })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    /// What the caller ends up with: the name it was found under, and the bytes.
    fn found(r: anyhow::Result<Option<Setting>>) -> Option<(String, String)> {
        r.unwrap().map(|s| (s.name, s.value))
    }

    #[test]
    fn the_new_name_is_the_only_name_a_fresh_install_needs() {
        assert_eq!(
            found(resolve(
                "KICKOFF_CHANNEL_TOKEN",
                value("new"),
                "HERDR_TG_TOKEN",
                None
            )),
            Some(("KICKOFF_CHANNEL_TOKEN".into(), "new".into()))
        );
    }

    #[test]
    fn an_existing_install_may_keep_the_old_name() {
        assert_eq!(
            found(resolve(
                "KICKOFF_CHANNEL_TOKEN",
                None,
                "HERDR_TG_TOKEN",
                value("old")
            )),
            // The refusal sentences quote this, so the OLD name must come back when the old name
            // is what the operator set.
            Some(("HERDR_TG_TOKEN".into(), "old".into()))
        );
    }

    #[test]
    fn two_identical_spellings_are_safe_during_a_staged_migration() {
        assert_eq!(
            found(resolve(
                "KICKOFF_CHANNEL_TOKEN",
                value("same"),
                "HERDR_TG_TOKEN",
                value("same")
            )),
            Some(("KICKOFF_CHANNEL_TOKEN".into(), "same".into()))
        );
    }

    #[test]
    fn two_different_spellings_are_refused_without_printing_either_value() {
        let error = resolve(
            "KICKOFF_CHANNEL_TOKEN",
            value("new-secret"),
            "HERDR_TG_TOKEN",
            value("old-secret"),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("KICKOFF_CHANNEL_TOKEN"));
        assert!(error.contains("HERDR_TG_TOKEN"));
        assert!(!error.contains("new-secret"));
        assert!(!error.contains("old-secret"));
    }
}

//! Opaque, non-interchangeable id newtypes.
//!
//! The compile-time half of D3: the catastrophic failure of a remote-control surface is words
//! landing in the wrong terminal, ids are plain strings on the wire, and `wC` / `wC:t1` / `wC:p1`
//! are trivially transposable. `send_text(&PaneId, …)` cannot be called with a `WorkspaceId`.
//!
//! There is deliberately NO `workspace_hint()` / id parsing. The `"<workspace>:<pane>"` shape holds
//! in all 6 live samples but herdr's schema types every id as an opaque string. Route from
//! [`crate::proto::model::PaneInfo::workspace_id`]; never parse an id.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// `#[serde(transparent)]`: on the wire this is a bare JSON string, exactly as herdr emits it.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps a raw wire id. No validation and no parsing: the schema constrains nothing
            /// beyond "string", so any shape herdr hands us must survive a round trip verbatim.
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// The raw wire id.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

opaque_id!(PaneId, "A herdr pane id, e.g. `wD:p1`.");
opaque_id!(WorkspaceId, "A herdr workspace id, e.g. `wD`.");
opaque_id!(TabId, "A herdr tab id, e.g. `wD:t1`.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_as_bare_strings() {
        let p = PaneId::new("wD:p1");
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"wD:p1\"");
        assert_eq!(
            serde_json::from_str::<PaneId>("\"wD:p1\"").unwrap(),
            PaneId::from("wD:p1")
        );
        assert_eq!(p.as_str(), "wD:p1");
        assert_eq!(p.to_string(), "wD:p1");
    }

    #[test]
    fn ids_of_different_kinds_are_distinct_types() {
        // This is the whole point of the module; the assertion that matters is that the line
        // `let _: PaneId = WorkspaceId::new("wD");` does not compile. Kept as a value check so the
        // types are at least exercised together.
        let w = WorkspaceId::new("wD");
        let t = TabId::new("wD:t1");
        let p = PaneId::new("wD:p1");
        assert_ne!(w.as_str(), p.as_str());
        assert_ne!(t.as_str(), p.as_str());
    }
}

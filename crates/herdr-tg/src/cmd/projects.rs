//! `herdr-tg projects` — every enrolled project, for a person at the keyboard or for a machine.
//!
//! `--json` is the read-only inventory `docs/CAPABILITIES.md` offered and kickoff's room-map
//! handshake reads topic ids from. Its shape is a promise, so it is built by hand rather than by
//! serialising the registry's own struct: that struct carries the secret's hash and the icon
//! colour, neither of which is anybody else's business, and its field order is whatever the struct
//! happens to be. What goes out is exactly the offered fields, in the offered order, and nothing
//! about the operator beyond the repo path the registry already holds — no chat id, no state
//! directory, no pid.
//!
//! `connected` and `connected_lanes` are the two fields the registry cannot answer, because only
//! the running hub holds the live claims map. They come from the snapshot the hub writes
//! (`presence.rs`), and they are `null` whenever no running hub stands behind that snapshot —
//! unknown, said as unknown, rather than a `false` nobody could prove.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use hub_proto::{LaneId, ProjectId};
use serde::Serialize;

use crate::hub::Addr;
use crate::registry::{Project, Registry};

/// One project, in the offered shape. Field order is the promise; `serde` keeps a struct's.
#[derive(Debug, Serialize)]
pub(crate) struct Row<'a> {
    pub project_id: &'a ProjectId,
    pub title: &'a str,
    pub repo: &'a Path,
    pub enabled: bool,
    /// `null` until a bridge has been live at least once. Honest about that, because a topic does
    /// not exist at enrolment and a handshake that assumed one would read a number that is not
    /// there.
    pub topic_id: Option<i32>,
    /// Whether the project's OWN voice has a bridge on the socket right now — the same fact the
    /// phone's `/projects` renders — or `null` when no running hub can say.
    pub connected: Option<bool>,
    /// Every worktree of it that has ever been given a topic, by the address it was given under.
    pub lanes: &'a BTreeMap<LaneId, i32>,
    /// The addresses of it that have a bridge on the socket right now, or `null` when no running
    /// hub can say.
    pub connected_lanes: Option<Vec<LaneId>>,
}

/// `herdr-tg projects`, either shape.
pub(crate) fn projects(json: bool) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    projects_in(
        &Registry::default_path(),
        &crate::lock::state_dir(),
        json,
        &mut out,
    )
}

/// The same, against a named registry and state directory, so it can be tested without touching
/// the operator's own.
fn projects_in(
    registry_path: &Path,
    state_dir: &Path,
    json: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    // Refused, never guessed: a registry that is there and cannot be read is not an empty one,
    // and `[]` on stdout with exit 0 is what a machine on the other end of the pipe acts on. A
    // file that does not exist yet is genuinely nothing enrolled, and `try_load` says so.
    let registry = Registry::try_load(registry_path).map_err(|e| match e {
        crate::registry::EnrolError::Unreadable { path, why } => anyhow::anyhow!(
            "I cannot read the list of enrolled projects at {}, so nothing is listed — an empty \
             list here would be taken for the truth. What went wrong reading it: {why}",
            path.display()
        ),
        other => anyhow::Error::from(other),
    })?;
    if json {
        let rows = inventory(&registry, state_dir);
        // Straight to the writer, never through a `serde_json::Value`: a `Value`'s object is a
        // sorted map, and the promised order is not alphabetical. Compact, because this is a
        // machine surface with `jq` on the other end of the pipe.
        serde_json::to_writer(&mut *out, &rows)?;
        out.write_all(b"\n")?;
        return Ok(());
    }
    let mut any = false;
    for p in registry.all() {
        any = true;
        let state = if p.enabled { "" } else { "  (switched off)" };
        // The middle column is 34 wide because "no topic of its own yet, 12 worktrees" is 37 at its
        // longest realistic value and a column that overruns takes the repo paths out of line.
        writeln!(
            out,
            "{:<24} {:<34} {}{}",
            p.title,
            where_it_talks(p),
            p.repo.display(),
            state
        )?;
    }
    if !any {
        writeln!(
            out,
            "Nothing is enrolled yet. Add a project with:  herdr-tg enroll <repo>"
        )?;
    }
    Ok(())
}

/// The inventory, from a registry and the state directory the running hub writes into.
///
/// Sorted by title, because the registry hands projects over in id order — a hashed string that
/// is neither alphabetical nor the order he enrolled them in — and a reader diffing two runs of
/// this wants the rows to stay put.
pub(crate) fn inventory<'a>(registry: &'a Registry, state_dir: &Path) -> Vec<Row<'a>> {
    // `None` is "no running hub could say", and it is carried as `None` all the way out: a
    // project's `connected` is then `null`, never `false`.
    let live: Option<BTreeSet<Addr>> = crate::presence::vouched_for(state_dir)
        .map(|s| s.connected.iter().map(|c| c.addr()).collect());
    let mut projects: Vec<&Project> = registry.all().collect();
    projects.sort_by(|a, b| {
        a.title
            .cmp(&b.title)
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
    projects
        .into_iter()
        .map(|p| Row {
            project_id: &p.id,
            title: &p.title,
            repo: &p.repo,
            enabled: p.enabled,
            topic_id: p.topic_id,
            connected: live
                .as_ref()
                .map(|l| l.contains(&Addr::project_itself(p.id.clone()))),
            lanes: &p.lane_topics,
            // The addresses live right now, in the same `null`-when-unvouched shape. `connected`
            // is the project's own voice only, and a repo whose sessions are all dispatched into
            // worktrees never has one — so without this the dispatcher read `false` for a project
            // whose agent was live, and could not tell idle from live-through-its-rooms.
            connected_lanes: live.as_ref().map(|l| {
                l.iter()
                    .filter(|a| a.project == p.id)
                    .filter_map(|a| a.lane.clone())
                    .collect()
            }),
        })
        .collect()
}

/// Which topics a project has, for the terminal listing.
///
/// The worktree count is here because this is the only VISIBLE sign a lane ever ran: a worktree's
/// topic outlives the worktree by design, and nothing else on the box names how many a project has
/// collected. Not a size warning — the file itself is cheap, and the measured numbers are on
/// `Project::lane_topics`.
fn where_it_talks(p: &Project) -> String {
    let topic = match p.topic_id {
        Some(id) => format!("topic {id}"),
        // Three states, not two. A repo whose sessions are all dispatched into worktrees never
        // binds a topic of its own, so "not connected yet" beside a worktree count is a row that
        // contradicts itself — and it is that repo's ORDINARY row, not an edge case.
        None if p.lane_topics.is_empty() => "not connected yet".to_owned(),
        None => "no topic of its own yet".to_owned(),
    };
    match p.lane_topics.len() {
        0 => topic,
        1 => format!("{topic}, 1 worktree"),
        n => format!("{topic}, {n} worktrees"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Enrol a folder of this name into the registry at `dir/projects.json`.
    fn enrol(dir: &Path, name: &str) -> Project {
        let folder = dir.join(name);
        std::fs::create_dir_all(&folder).expect("dir");
        Registry::load(dir.join("projects.json"))
            .enrol(&folder)
            .expect("enrols")
            .0
    }

    /// The JSON exactly as the command prints it.
    fn as_json(rows: &[Row<'_>]) -> String {
        serde_json::to_string(rows).expect("json")
    }

    /// A surface that must never be reached. The inventory reads state; it sends nothing.
    struct Nobody;
    impl crate::hub::Surface for Nobody {
        async fn create_topic(&self, _: &str, _: u8) -> Result<i32, crate::hub::Refused> {
            unreachable!("the inventory creates no topics")
        }
        async fn send(
            &self,
            _: i32,
            _: &str,
            _: &[hub_proto::AskOption],
            _: Option<&hub_proto::MsgId>,
        ) -> crate::hub::SendOutcome {
            unreachable!("the inventory sends nothing")
        }
        async fn say_in_general(&self, _: &str) -> crate::hub::SendOutcome {
            unreachable!("the inventory says nothing")
        }
        async fn rewrite(&self, _: &hub_proto::MsgId, _: &str) -> anyhow::Result<()> {
            unreachable!("the inventory rewrites nothing")
        }
        async fn retire_buttons(
            &self,
            _: i32,
            _: &hub_proto::MsgId,
            _: &str,
            _: &str,
        ) -> anyhow::Result<()> {
            unreachable!("the inventory retires nothing")
        }
        async fn mark(
            &self,
            _: i64,
            _: &hub_proto::MsgId,
            _: crate::hub::Mark,
        ) -> Result<(), crate::hub::Refused> {
            unreachable!("the inventory marks nothing")
        }
    }

    /// A hub whose state files all live in `dir`, which is what the running hub's look like.
    fn a_hub_in(dir: &Path) -> Arc<crate::hub::Hub<Nobody>> {
        Arc::new(crate::hub::Hub::new(
            Arc::new(Nobody),
            Registry::load(dir.join("projects.json")),
            crate::hub::AskLedger::load(dir.join("asks.json")),
            crate::hub::HubAudit::new(dir.join("hub.audit.log")),
            vec![-1001],
            -1001,
        ))
    }

    #[test]
    fn projects_json_says_null_for_a_topic_that_does_not_exist_yet() {
        // A topic is made on the first LIVE connection, not at enrolment, so for the whole of the
        // time between the two there is no number to give. A handshake that reads this must be able
        // to see that, and `0` or an empty string would each be a number that is not there.
        let d = tempfile::tempdir().expect("tmp");
        let p = enrol(d.path(), "fresh");
        let registry = Registry::load(d.path().join("projects.json"));
        let json = as_json(&inventory(&registry, d.path()));
        assert!(
            json.contains("\"topic_id\":null"),
            "a project that has never connected has a topic id: {json}"
        );

        // And once one exists, it is the number.
        let mut r = Registry::load(d.path().join("projects.json"));
        r.bind_topic(&Addr::project_itself(p.id.clone()), 4242)
            .expect("binds");
        r.bind_topic(
            &Addr::lane_of(p.id.clone(), LaneId::new("lane-0905-120000-1")),
            4243,
        )
        .expect("binds");
        let json = as_json(&inventory(&r, d.path()));
        assert!(json.contains("\"topic_id\":4242"), "{json}");
        assert!(
            json.contains("\"lanes\":{\"lane-0905-120000-1\":4243}"),
            "a lane's topic is not listed under its address: {json}"
        );
    }

    #[tokio::test]
    async fn projects_json_says_connected_only_from_the_live_claims_map() {
        // The only source of "connected" is the claims map inside the running hub. A topic binding
        // is permanent from a project's first connection on and says nothing about now; the phone's
        // own list stopped rendering it for that reason, and this surface must not start.
        let d = tempfile::tempdir().expect("tmp");
        let idle = enrol(d.path(), "llm-gateway");
        let busy = enrol(d.path(), "herdr-tg");
        Registry::load(d.path().join("projects.json"))
            .bind_topic(&Addr::project_itself(idle.id.clone()), 1001)
            .expect("binds");

        // This process IS the running hub: it holds the claim and it is named in the lock file,
        // exactly as `serve` leaves things.
        let hub = a_hub_in(d.path());
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        hub.claim(
            Addr::project_itself(busy.id.clone()),
            std::process::id(),
            "i1".into(),
            tx,
        )
        .await
        .expect("claims");
        std::fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");

        let registry = Registry::load(d.path().join("projects.json"));
        let rows = inventory(&registry, d.path());
        let row = |title: &str| {
            rows.iter()
                .find(|r| r.title == title)
                .unwrap_or_else(|| panic!("no row for {title}"))
        };
        assert_eq!(
            row(&busy.title).connected,
            Some(true),
            "a project whose bridge is on the socket is not connected: {}",
            as_json(&rows)
        );
        assert_eq!(
            row(&idle.title).connected,
            Some(false),
            "a project that merely has a topic from a previous run is connected: {}",
            as_json(&rows)
        );

        // The bridge goes away: the answer follows the map, not the file it left behind.
        hub.release(&Addr::project_itself(busy.id.clone()), std::process::id())
            .await;
        let rows = inventory(&registry, d.path());
        assert_eq!(
            rows.iter()
                .find(|r| r.title == busy.title)
                .map(|r| r.connected),
            Some(Some(false)),
            "a project whose bridge has gone is still connected: {}",
            as_json(&rows)
        );

        // No running hub — the lock names nobody alive — and the answer is UNKNOWN, for every
        // project, never false: nothing here can prove what a hub that is not running knows.
        std::fs::remove_file(d.path().join("hub.lock")).expect("no hub");
        let rows = inventory(&registry, d.path());
        assert!(
            rows.iter().all(|r| r.connected.is_none()),
            "with no hub running the inventory still claims to know who is connected: {}",
            as_json(&rows)
        );
        assert!(
            as_json(&rows).contains("\"connected\":null"),
            "unknown is not written as null: {}",
            as_json(&rows)
        );
    }

    #[tokio::test]
    async fn projects_json_carries_no_chat_id_and_no_path_but_the_repos() {
        // This is read by another org's tooling and lands in its logs. The forum's chat id is the
        // one number that, with the bot's name, lets a stranger find the operator's forum; the
        // state directory and the socket path name his home. Neither is the inventory's to give.
        let d = tempfile::tempdir().expect("tmp");
        let p = enrol(d.path(), "one");
        let hub = a_hub_in(d.path());
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        hub.claim(
            Addr::project_itself(p.id.clone()),
            std::process::id(),
            "i1".into(),
            tx,
        )
        .await
        .expect("claims");
        std::fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");
        let mut r = Registry::load(d.path().join("projects.json"));
        r.bind_topic(&Addr::project_itself(p.id.clone()), 77)
            .expect("binds");

        let rows = inventory(&r, d.path());
        let json = as_json(&rows);
        assert!(
            !json.contains("-1001"),
            "the forum's chat id leaked: {json}"
        );
        assert!(!json.contains("chat"), "a chat is named: {json}");
        assert!(!json.contains("token"), "the secret's hash leaked: {json}");
        assert!(!json.contains("icon"), "a field nobody promised: {json}");
        assert!(!json.contains("pid"), "a pid leaked: {json}");

        // Every string that looks like a path is a repo path the registry already holds.
        let repos: BTreeSet<String> = r.all().map(|p| p.repo.display().to_string()).collect();
        let doc: serde_json::Value = serde_json::from_str(&json).expect("json");
        fn strings(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::String(s) => out.push(s.clone()),
                serde_json::Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
                serde_json::Value::Object(o) => o.iter().for_each(|(k, v)| {
                    out.push(k.clone());
                    strings(v, out);
                }),
                _ => {}
            }
        }
        let mut all = Vec::new();
        strings(&doc, &mut all);
        for s in all.iter().filter(|s| s.contains('/')) {
            assert!(
                repos.contains(s),
                "a path that is not an enrolled repo's reached the inventory: {s}\n{json}"
            );
        }
    }

    #[test]
    fn the_fields_come_in_the_promised_order_and_the_rows_in_title_order() {
        // The shape is the contract, and a reader diffing two runs wants rows that stay put. The
        // registry hands projects over in id order — a hash — so the order here is chosen.
        let d = tempfile::tempdir().expect("tmp");
        enrol(d.path(), "zeta");
        enrol(d.path(), "alpha");
        enrol(d.path(), "mid");
        let registry = Registry::load(d.path().join("projects.json"));
        let rows = inventory(&registry, d.path());
        let titles: Vec<&str> = rows.iter().map(|r| r.title).collect();
        assert_eq!(titles, vec!["alpha", "mid", "zeta"]);

        let json = serde_json::to_string(&rows[0]).expect("json");
        let at = |key: &str| {
            json.find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("{key} is missing: {json}"))
        };
        let order = [
            "project_id",
            "title",
            "repo",
            "enabled",
            "topic_id",
            "connected",
            "lanes",
            "connected_lanes",
        ];
        for pair in order.windows(2) {
            assert!(
                at(pair[0]) < at(pair[1]),
                "{} does not come before {}: {json}",
                pair[0],
                pair[1]
            );
        }
        assert!(json.starts_with("{\"project_id\":"), "{json}");
    }

    #[tokio::test]
    async fn projects_json_says_which_worktrees_are_live_when_the_project_itself_is_not() {
        // `connected` is the project's OWN voice, and a project whose sessions are all dispatched
        // into worktrees — every address kickoff mints — never has one. It read `false` while its
        // agent was live in a worktree, and nothing in the document could carry the difference
        // between idle and live-through-its-rooms; the phone's own list makes exactly that
        // distinction. The eighth field is the addresses live right now, under the same rule as
        // `connected`: unknown when no running hub can vouch for it.
        let d = tempfile::tempdir().expect("tmp");
        let p = enrol(d.path(), "oc-dogfood");
        let hub = a_hub_in(d.path());
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let lane = LaneId::new("lane-0905-120000-1");
        hub.claim(
            Addr::lane_of(p.id.clone(), lane.clone()),
            std::process::id(),
            "i1".into(),
            tx,
        )
        .await
        .expect("claims");
        std::fs::write(d.path().join("hub.lock"), std::process::id().to_string()).expect("lock");

        let registry = Registry::load(d.path().join("projects.json"));
        let rows = inventory(&registry, d.path());
        let row = &rows[0];
        assert_eq!(row.connected, Some(false), "{}", as_json(&rows));
        assert_eq!(
            row.connected_lanes.as_deref(),
            Some(&[lane.clone()][..]),
            "the worktree that is live right now is not listed: {}",
            as_json(&rows)
        );
        assert!(
            as_json(&rows).contains("\"connected_lanes\":[\"lane-0905-120000-1\"]"),
            "{}",
            as_json(&rows)
        );

        // No running hub: unknown, for the lanes as for the project.
        std::fs::remove_file(d.path().join("hub.lock")).expect("no hub");
        let rows = inventory(&registry, d.path());
        assert!(rows[0].connected.is_none() && rows[0].connected_lanes.is_none());
        assert!(
            as_json(&rows).contains("\"connected_lanes\":null"),
            "{}",
            as_json(&rows)
        );
    }

    #[test]
    fn a_registry_that_cannot_be_read_is_refused_rather_than_reported_as_nothing_enrolled() {
        // `Registry::load` starts EMPTY on any read error — the right decision for the hub's
        // boot, where refusing every project beats taking the channel down. On this surface the
        // same decision printed `[]` with exit 0 for a corrupt, unreadable or directory-shaped
        // file, and the reader is a machine that captures stdout and drops stderr: kickoff's
        // handshake learned that no project and no topic existed. A file that is there and cannot
        // be read is not an empty inventory; only a file that is not there is.
        let d = tempfile::tempdir().expect("tmp");
        let path = d.path().join("projects.json");

        let mut out = Vec::new();
        projects_in(&path, d.path(), true, &mut out).expect("nothing enrolled is an answer");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "[]\n",
            "a registry that does not exist yet is an empty inventory"
        );

        std::fs::write(&path, "{\"broken").expect("write");
        let mut out = Vec::new();
        let said = projects_in(&path, d.path(), true, &mut out)
            .expect_err("a corrupt registry was reported as an inventory");
        assert!(out.is_empty(), "something reached stdout: {out:?}");
        assert!(
            said.to_string().contains("cannot read"),
            "the refusal does not say the registry could not be read: {said}"
        );

        std::fs::remove_file(&path).expect("rm");
        std::fs::create_dir(&path).expect("a directory where the file should be");
        let mut out = Vec::new();
        projects_in(&path, d.path(), true, &mut out)
            .expect_err("a directory in place of the registry was reported as an inventory");
        assert!(out.is_empty(), "something reached stdout: {out:?}");

        // The table for a person refuses the same way: a listing that says "nothing is enrolled"
        // over a file it could not read sends him to enrol everything again.
        let mut out = Vec::new();
        projects_in(&path, d.path(), false, &mut out)
            .expect_err("the table said something over a registry it could not read");
        assert!(out.is_empty(), "something reached stdout: {out:?}");
    }

    #[test]
    fn the_terminal_listing_says_how_many_worktrees_a_project_has_collected() {
        // Every lane that ever went live keeps its topic for good, and the count lives in the file
        // the hub re-reads on every admission. This is the only view where that is visible at all,
        // and "twelve a day, never deleted" is a number the operator agreed to without ever being
        // shown it.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = Registry::load(d.path().join("projects.json"));
        let dir = d.path().join("herdr-tg");
        std::fs::create_dir_all(&dir).expect("dir");
        let (p, _) = r.enrol(&dir).expect("enrols");
        let addr = Addr::project_itself(p.id.clone());
        r.bind_topic(&addr, 1001).expect("binds");

        let said = |r: &Registry| where_it_talks(r.get(&p.id).expect("the project"));
        assert_eq!(said(&r), "topic 1001", "{}", said(&r));

        r.bind_topic(
            &Addr::lane_of(p.id.clone(), LaneId::new("lane-0902-201212-1")),
            1002,
        )
        .expect("binds");
        assert_eq!(said(&r), "topic 1001, 1 worktree", "{}", said(&r));

        r.bind_topic(
            &Addr::lane_of(p.id.clone(), LaneId::new("lane-0902-204418-2")),
            1003,
        )
        .expect("binds");
        assert_eq!(said(&r), "topic 1001, 2 worktrees", "{}", said(&r));

        for jargon in ["lane_topics", "Some", "None", "BTreeMap"] {
            assert!(
                !said(&r).contains(jargon),
                "jargon in the listing: {}",
                said(&r)
            );
        }
    }

    #[test]
    fn a_project_reached_only_through_its_worktrees_is_not_listed_as_never_connected() {
        // A repo whose sessions are all dispatched into worktrees never binds a topic of its own,
        // so `topic_id` stays empty for ever. "not connected yet" beside a worktree count is a row
        // that contradicts itself, and it is the ORDINARY row for such a repo, not an edge case.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = Registry::load(d.path().join("projects.json"));
        let dir = d.path().join("oc-dogfood");
        std::fs::create_dir_all(&dir).expect("dir");
        let (p, _) = r.enrol(&dir).expect("enrols");
        r.bind_topic(
            &Addr::lane_of(p.id.clone(), LaneId::new("lane-0902-231907-1")),
            1002,
        )
        .expect("binds");

        let said = where_it_talks(r.get(&p.id).expect("the project"));
        assert!(
            !said.contains("not connected yet"),
            "a project with a worktree topic is listed as one that has never connected: {said}"
        );
    }
}

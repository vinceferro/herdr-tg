<!-- PROPOSAL, 3 September 2026. NOT BUILT. No production code was changed to write this; every
     file:line below was read at HEAD e2ec3fb and every measurement was taken on this box. It is one
     design synthesised from four, and each idea taken from a proposal that was not the base names
     the proposal it came from. -->

# Conversations

## 1. The design, in three sentences

A conversation stops being a function of the directory a session launched in and becomes a **row the
operator minted at a terminal**, whose secret lives in a space the channel owns outside every repo —
one directory per conversation, `secret` and an optional `title` inside it.

A seed keeps working with no configuration because the repo it was opened in points at its
conversation through a **link the channel wrote**, instead of through a walk up the tree to a
gitignored file; a room is handed **which conversation it is** by whoever dispatched it, so three
rooms in one repo are three conversations instead of one conversation and two lockouts; a lane is
untouched, still an opaque name on `hello`, still free and unbounded inside the conversation its
secret already proved.

The hub does not change how it decides who is connecting: it still hashes the bytes a bridge
presented and compares them constant-time against `token_sha256`, and because those bytes are
**copied rather than rotated** it cannot tell — and never needs to tell — which file the bridge read
them out of.

**Base: "The blank book."** Grafts, each attributed where it appears: copy-never-rotate and the
path-segment containment assert from **"The conversation record"**; the per-conversation directory,
the repo→conversation link as *data rather than an algorithm*, and the fan-in re-key from **"A
conversation is a directory"**; the terminal gate on the door, the mount-the-directory rule and the
container admission argument from **"The door is the credential."**

---

## 2. How identity works, and why a message still cannot grant itself access

### The answer first

**Identity does not change.** `Registry::resolve` (`registry.rs:309`) still hashes the presented
token and compares it against every row with `subtle::ConstantTimeEq`, still deliberately does not
`break` (`registry.rs:313`). `Hub::admit` (`hub.rs:909`) still runs its gates in the same order:
peer uid (`hub.rs:916`), version, first-frame-is-`hello`, a fresh re-read of the registry
(`hub.rs:946`), the token (`hub.rs:952`), `enabled`, and only then the lane (`hub.rs:961-971`) — so a
caller holding no valid secret still learns only "unknown project". `Addr` (`hub.rs:719`) is still
built from the RESOLVED project plus the wire's lane (`hub.rs:977`), and the wire's `project_id`,
`repo` and `pid` are still ignored (`hub.rs:974`).

**Exactly one thing moves: which file the BRIDGE opens before it dials.**

### Why there is no second source in `resolve`, and this is the load-bearing simplification

Measured: `token_sha256` has **one reader in the whole tree** (`registry.rs:313`) and **one writer**
(`registry.rs:450`). The hub never opens a secret file — the bridge does, and puts the bytes on the
wire. So moving the secret from `<repo>/.kickoff/hub.token` into the channel space is not a change to
the hub at all, provided the bytes are the same bytes.

This is the graft from **"The conversation record"**, and it deletes what all four proposals
independently named as their riskiest change: a two-source `Registry::resolve` running while the hub
is live, whose silent failure mode is one agent's question arriving in another agent's topic. There
is no fallback branch, no precedence rule, and no compatibility code in the hub, because
`projects.json` remains the single store of hashes and the channel space holds only the bytes.

Blank slots minted for rooms are ordinary rows in that same store, with their own `token_sha256`,
written by the same terminal door under the same flock (`registry.rs:517`).

### Why a message still cannot grant itself access

**Literally, by construction.** `crates/hub-proto/` is byte-identical. No frame in this design
creates a conversation, mints a credential or grants access, so there is nothing for an adapter —
the untrusted side of seam ① — to say. Kickoff never speaks to the hub about opening a room, so
"kickoff is trusted" is not needed and is not offered.

**And now the correction the sceptic deserves, because the constraint as usually stated is already
false in shipped code.** A `hello` naming a lane nobody has ever seen creates a forum topic and binds
it. Verified: `bind_topic` (`registry.rs:345`) has exactly **one production caller** — `hub.rs:1406`,
inside `topic_for` (`hub.rs:1340`) — and `topic_for` is reached only from a live connection. Every
other caller in the tree is a test. There is no uniqueness check and no allowlist;
`lane_is_addressable` (`hub.rs:773`) is shape-only and every clause of it is justified by
audit-forging rather than by meaning. Two of the five live topic bindings on this box (284 and 286)
are lane topics, and the only path that can have made them is a `hello`. So "a message cannot create
a conversation" has not been true since 2 September, and a design that recites it is reasoning about
a system that does not exist. Both **"The blank book"** and **"A conversation is a directory"** found
this independently; the restatement below is theirs.

**The line the code actually holds is narrower and sharper:**

> Inbound content may address things inside a trust domain a secret has already proved. It may never
> widen that domain, and it may never mint or read a credential.

`docs/CAPABILITIES.md` states the same thing from the other side — the hub allocates nothing,
uniqueness within a project belongs to whoever dispatches, and REFUSES #1 is "naming another
project's conversation".

**Held against that line, this design does not move it.** Every slot exists as a row in
`projects.json` before any dispatcher runs, put there by `herdr-tg grant` at a keyboard. When kickoff
opens a room it **takes a file** — an `O_EXCL` rename of a slot the operator minted — and sends no
message to anything. The hub then sees an ordinary `hello` carrying a secret it enrolled days ago,
from a process it cannot distinguish from any other. The dispatcher SELECTS from what the machine
already knows and NAMES nothing new. It cannot mint an N+1th slot, cannot reach another seed's grant
directory, cannot enrol, cannot flip `enabled`.

What changes is only what a terminal grant CONTAINS. Today one enrolment buys one conversation plus
**unbounded** lanes of it, each of which mints a permanent topic from a wire string with no
allowlist. After this it buys N conversations plus unbounded lanes of each. A book of N is therefore
a **narrowing** of what already ships in the dimension that is currently infinite — that framing is
**"The blank book"**'s and it is the honest reason a non-keyboard room birth is defensible at all.

### Argv is not a person

`herdr-tg open` and `herdr-tg grant` are gated on `std::io::stdin().is_terminal()`, not merely on
being argv. The precedent is already in the tree and it was paid for: `enroll.rs:19` passes that fact
in, and `enroll.rs:38-52` explains why — *"the likeliest reader of this refusal is an autonomous
agent that was just told to run this command."* This graft is from **"The door is the credential"**,
which made the same point about `open` and then did not apply it to itself.

### What a compromised dispatcher gets, stated rather than hidden

The grant directory plus the keyring is N rooms. Whoever holds them opens N topics on his phone and
speaks as those rooms. Bounded to N; cannot reach a conversation outside the book; cannot enrol;
cannot flip `enabled`; cannot mint an N+1th. That is a real and new shape of loss that a single repo
token did not have, and it is the price of the book.

### The one thing that genuinely widens, named rather than buried

A room needs to arrive on his phone **named after its function**, not as "room 3". Titles live in
`projects.json`, which the dispatcher must not write. So the conversation directory carries an
optional `title` file, and `topic_for` prefers it over `p.title` when minting the topic
(`hub.rs:1346-1349`). That moves title authorship from "a terminal only" to "whoever holds the slot".

Four things keep it small, and the fourth is the reason it is acceptable:

* It is **not on the wire.** Seam ①'s first sentence — *"Identity is a secret, never a name; `hello`
  carries no display name"* — stays exactly true, and `registry.rs:1-9` still holds.
* It is **display only.** Routing keys on `Addr` alone; the title is read once, at topic creation.
* It is **shape-refused** on the same clauses as `lane_is_addressable` (`hub.rs:773`): non-empty, no
  control character, capped at `MAX_TITLE`. An unreadable or refused title falls back to the
  registry's.
* It is **written once and never re-set.** Verified: the hub calls `create_forum_topic`
  (`surface.rs:77-82`) and nothing else — there is no `editForumTopic` call site in the binary, and
  `docs/HUB-DESIGN.md:180` records that as a deliberate refusal — so if the operator renames a topic
  on his phone, the rename sticks for ever.

The alternative is to ship no title file, let the machine name the slot at grant time, and let him
rename each topic once. That is question 2 in §7 and it is his to answer, not mine.

---

## 3. How a seed, a room and a lane each get a conversation

### The space, and what is in it

One derivation, beside `lock::state_dir()` (`lock.rs:39`), overridable with `HERDR_TG_CHANNEL_HOME`:

```
<channel home>/                                   0700, re-asserted after creation
  conversations/<conv-id>/                        0700
    secret                                        0600   the bytes a bridge presents
    title                                         0600   display only, optional
  by-repo/<sha256(canonical main worktree)[..16]> 0600   one line: a conv-id
  grants/<seed-conv-id>/
    000.vacant  →  000.taken                      0600   one line: a conv-id
```

A conv-id is `p-<12 hex>` for the rows that already exist and `c-<12 hex>`, randomly minted, for
everything new. It must be `^[pc]-[0-9a-f]{12}$` — checked at the door, before a byte is written,
because it becomes a path segment.

**The secret lives in exactly one place.** A grant slot holds an id, never a credential, so rotation
touches one file and there is no second credential home to keep in step. (Both the security and the
taxonomy reviewers flagged the two-homes version of the book as unreconciled; this is the fix.)

**0700 is step zero, not a detail.** Measured on this box: the service runs with `UMask=0022`, a bare
`create_dir_all` under that umask produces `755`, and both existing call sites are bare
(`lock.rs:120`, `registry.rs:553`). Today's state directory is `0700` **by history, not by code**. The
mode must also be re-asserted after creation, for the reason `write_token_file` already records at
`registry.rs:603-605`: `.mode()` applies only at creation, so an existing directory keeps whatever it
had.

### How a bridge answers "which conversation am I" — four terms, strict order

This replaces `findProject`/`searchUpward` (`where.ts:135-158`).

1. **`KICKOFF_CONVERSATION=<conv-id>`**, set by whoever dispatched. Read
   `<channel home>/conversations/<id>/secret`. **Set but unreadable is a permanent refusal** — never
   a fall-through, because a fall-through under a failed bind-mount is a session speaking as the
   wrong conversation.
2. **The repo's default binding.** Ask git for the top of the working tree exactly as today, hash it,
   read `by-repo/<hash>` → conv-id → secret. This is the graft from **"A conversation is a
   directory"**: *"which conversation does this repo default to" stops being an ALGORITHM and becomes
   DATA*, and that single move is what makes a second conversation in one repo expressible at all.
3. **Legacy** — the upward walk to `<repo>/.kickoff/hub.token`, kept for the whole migration so an
   old bridge and a new one both work, deleted in the last step.
4. **Nothing** → refuse permanently, naming the terminal verb, **with no repo path in the sentence.**

Term 2 is keyed on the **main** working tree, which a linked worktree already computes from
`--git-common-dir` (`where.ts:83-84`). So the cross-boundary hop at `where.ts:143` disappears: a lane
worktree and its main tree read one credential by construction rather than by a special case, and one
whole class of "this project is not enrolled" goes away. `the_real_plugin_finds_its_project_from_
inside_a_lane_worktree_of_it` (`hub/tests.rs:1916`) gets rewritten to say that.

Term 4 is also what closes TAXONOMY #6. `enrolHint` (`server.ts:489-494`) names a directory and its
advice mints a second project when the session is in a lane worktree; with no repo path to print the
sentence becomes unwritable rather than fixed.

### A seed

`herdr-tg open <repo> [--title <name>]`, at a terminal. Mints the row, writes
`conversations/<id>/secret`, writes `by-repo/<hash of that repo's main tree>` → that id, prints the
one line a launcher needs. **Writes nothing into any repo.**

A session opened by hand in that repo, with no environment variable at all, lands on term 2 and
behaves exactly as it does today.

### A room — the case that does not work today

Today, three rooms as plain directories of one org repo present the identical `(secret, no lane)`
pair: `factsFor` sets a lane only inside a linked worktree (`where.ts:86`), `findProject` walks up to
the org root's token, and the second and third arrivals are refused `already_claimed`. Nor can a room
be its own project: `enrol` refuses any folder strictly below an enrolled one (`registry.rs:412`).
One conversation, two lockouts.

Under this design:

1. **Once, at a keyboard:** `herdr-tg grant <seed> --rooms 8`. Eight complete conversations are
   minted — eight rows with `enabled: true`, `topic_id: None`, eight secrets in the keyring, eight
   `NNN.vacant` slot files under `grants/<seed>/`. A vacant slot is genuinely free: a topic binds only
   on first LIVE connection (`registry.rs:63`, `hub.rs:1340`), so it costs a directory and a row and
   never appears on his phone.
2. **The seed's conversation proposes a function.** He taps yes.
3. **The dispatcher takes the next slot** — an `O_EXCL` rename `000.vacant` → `000.taken`, so two
   dispatchers cannot take one — reads the conv-id out of it, writes `title` into that conversation's
   directory, and starts the engine with `KICKOFF_CONVERSATION=<that id>`.
4. **The room dials.** The hub sees a `hello` with a secret it enrolled days ago and a lane it never
   sent. `admit` resolves it to its own row, `topic_for` mints a topic named after the function, and
   the greeting is `"<title> is connected."` — the project branch at `hub.rs:1425`, not the lane
   branch at `hub.rs:1427`.

A room is therefore **its own top-level conversation**: its own claim, its own topic, its own
ledger — a sibling of the seed, which is what the taxonomy says it is. It never lands on the seed's
address, so TAXONOMY #1 closes at the root: the address stops being a function of the launch
directory.

It also means a room never touches the lane namespace, so the three strings that say "worktree"
(`hub.rs:1427`, `bot.rs:500`, `bot.rs:521`) stay **true** rather than needing a rewrite. That
observation is **"The conversation record"**'s and it is the reason this design refuses to model a
room as a lane.

**A slot is spent, never recycled.** Re-using a slot id would inherit the dead room's topic and its
whole rolling history — the exact defect `registry.rs:421` warns about in as many words. A book is
consumed and refilled at a terminal.

**Vacant slots are hidden from `/projects`** by one predicate over `registry.all()` (`bot.rs:479`):
a row that is vacant and has no topic is not listed. `Project` gains one field for it,
`#[serde(default)] vacant: bool`, added the way `lane_topics` was (`registry.rs:88`) so a registry
written by an older build still parses. Nothing ever clears the field — the moment a topic binds, the
row appears — so no new writer is introduced.

### A lane

**Unchanged, and free.** Still `hello.lane`, still git's own name for the worktree, still an address
inside a trust domain the secret already proved, still costing no slot. Both a seed and a room can
have lanes.

That split is the design's economics and it is derived from the trust boundary rather than from
convenience: `admit` builds `Addr` from the resolved project plus the wire's lane (`hub.rs:977`), so a
lane genuinely cannot widen reach while a new conversation genuinely can. Rooms are a handful a week
and cost a terminal-minted slot; lanes are a dozen a day and cost nothing.

### The fan-in must be re-keyed in the same slice, or the room case breaks on day one

`faninSocket` hashes `(mainTop, lane)` (`where.ts:115-118`, `fanin.ts:74`, chosen at
`server.ts:130-134`). Two rooms in one repo are both top-level with `lane: null`, so both derive the
**identical** relay socket: the relay re-checks the secret and turns the second away, and the second
room's opencode path is dead the first time the feature is used for what it is for. Re-key onto the
conversation id — which this design supplies — and TAXONOMY #5's monorepo collision closes by
construction as well. Both **"The conversation record"** and **"A conversation is a directory"**
identified this and scheduled it out; the migration reviewer was right that shipping without it
manufactures a known-fatal condition, so it is in.

---

## 4. What changes on the wire, and whether his live session survives

### The wire

**Nothing. Zero frames.** Nine up, six down, `VERSION` still 1 (`frame.rs:36`). `crates/hub-proto/`
is not touched at all.

`Hello` keeps its exact fields (`frame.rs:172-196`), `lane` keeps its `skip_serializing_if`
(`frame.rs:194-195`), and `Welcome`'s lane echo (`frame.rs:296-297`) is untouched, so the
new-bridge/old-hub guard that seam ① needed for lanes goes on working. No new field, no new kind, no
version bump, no capability.

Two things on the wire this design deliberately refuses to start using: `hello.repo`
(`frame.rs:180`), which is unauthenticated free text that `hub.rs` reads nowhere and which would
become an attack surface the moment anything routed on it; and `hello.pid`, which stays discarded
(`hub.rs:974`) in favour of the socket's own.

### The bridge running in his session right now

**It survives by doing nothing, and needs no coordination at any step.**

`plugins/kickoff-channel/server.ts` reads `<repo>/.kickoff/hub.token` through `findProject`
(`server.ts:533`, `where.ts:135`) and presents those bytes. The migration **copies** those bytes into
the keyring and **leaves the repo file exactly where it is**; `token_sha256` is never rewritten, so
the token that session is holding keeps resolving through every step. That file is removed only in
step 7, per project, after that project's bridge has reconnected on the new path — which for a Claude
session means after the session has ended anyway, because a channel plugin restarts only when its
session does.

This is why copy-never-rotate is the graft that matters most. **"The blank book"** as written rotated
`token_sha256` in place while also promising the old repo token kept resolving; those cannot both be
true, and the bridge the contradiction would have killed is the one in his running session, which
cannot restart to recover. Copying removes the contradiction rather than sequencing around it.

An old bridge against a new hub works throughout. A new bridge against an old hub also works, because
term 3 of the ladder is the legacy walk, so there is no ordering constraint between deploying the hub
and deploying a bridge.

`the_real_plugin_and_the_real_hub_agree_on_the_wire` (`hub/tests.rs:1598`) must stay green at every
step. This design predicts it needs no change to its assertions — only to how its fixture plants a
secret (`hub/tests.rs:1610`).

---

## 5. The migration — three projects, five bindings

**Measured at HEAD e2ec3fb, read-only:** three enrolled projects, and **five** topic bindings, not
four — `herdr-tg` → 253, `hub-dogfood` → 255 with lane topics `hub-dogfood-lane-a` → 284 and
`hub-dogfood-lane-b` → 286, and `oc-dogfood` → 267. All three rows are `enabled`.

**The asset:** topic bindings hang off `ProjectId`, and `ProjectId` does not move. The conversation
directory is *named by* the id the registry already holds, so all five come across for free with no
schema change to `projects.json` and no re-derivation. Every proposal agreed on the trap and it is
worth repeating: the migration must **never** route through `Registry::enrol`, which re-derives the id
from the canonical repo path (`registry.rs:421`). One wrong call orphans five topics — permanently,
undeletably, carrying his history.

0. **The mode, first and alone.** Create the channel home and every directory under it at `0700`, and
   re-assert after creation (`registry.rs:603-605` is the precedent). Measured precondition, not a
   hypothetical: `UMask=0022`, bare `create_dir_all` → `755`. Nothing else may land before this,
   because everything after it puts credentials there. No live effect; reversible.
1. **Widen additively.** `conversations/`, `by-repo/`, `grants/` created. Nothing reads them.
2. **`herdr-tg adopt-secrets`** — one shot, idempotent, dry-run by default, **no service touch**. Per
   row: create `conversations/<its existing id>/`, copy the bytes of `<repo>/.kickoff/hub.token` into
   `secret` at 0600, write `by-repo/<sha256 of that repo's canonical main tree>` → that id.
   **`projects.json` is not written at all**, and `token_sha256` does not change. A crash halfway
   leaves a second copy of a secret nobody is reading yet. Rollback is `rm -rf` on the new tree.
3. **The terminal doors.** `open` and `grant`, both `is_terminal()`-gated, writing the new place. The
   git guard (`enroll.rs:193`, and the refusal at `enroll.rs:38-60`) **stays** — it is the only
   protection for the repos still on the old path.
4. **`topic_for` learns the title file** (`hub.rs:1346-1349`), shape-refused, falling back to
   `p.title`. RED first. Note that a second place composes a display title for the log and the audit
   subject (`hub.rs:1664-1672`); both must go through one function or they will disagree about what a
   room is called.
5. **`/projects` hides vacant slots** — one predicate at `bot.rs:479-504`, or his fleet list grows
   eight rows of nothing.
6. **The bridges, last, because a channel plugin restarts only with its session.** The four-term
   ladder in `where.ts` with the legacy walk as term 3; `where.ts:143` deleted; the enrol hints at
   `server.ts:489-494` and `server.ts:533-539` rewritten to name a verb rather than a path;
   `bridge.ts:41` and `bridge.ts:185-190` given the same ladder; `faninSocket` re-keyed onto the
   conversation id (`where.ts:115-118`, `fanin.ts:74`, `server.ts:130-134`).
7. **Delete the repo token files, one project at a time, at his pace.** For each: confirm its bridge
   has reconnected on the new path, then `rm <repo>/.kickoff/hub.token`. This is the first
   irreversible step and it is per-project. Rollback before it is `herdr-tg enroll <repo>`, and the
   topic comes back because the id never moved.
8. **Only then delete the old path:** `TOKEN_FILE` (`registry.rs:37`), `write_token_file`
   (`registry.rs:581`), the git guard, `--even-if-git-would-commit-it`, term 3 of the ladder, and the
   **29 references to `.kickoff/hub.token` across nine files** (measured: `hub/tests.rs` ×10,
   `test-against-a-fake-hub.ts` ×6, `server.ts` ×3, `enroll.rs` ×3, `registry.rs` ×2, `where.ts` ×2,
   `bridge.ts`, `test-harness.ts`, `test-against-fakes.ts`). Retire the three orphan state files while
   in there: two of them still hold the forum id and the operator's own user id at rest.

**The prize lands at step 7, and it is measured rather than argued.** Of the 13 adopted repos under
`~/Projects`, **10 would not have git ignore `.kickoff/hub.token`** — checked with `git check-ignore`,
just now. Today the enrol door refuses those outright, so going from three projects to fourteen means
editing ten `.gitignore` files in trees whose own coordinators commit and push unattended, or typing
an override flag at a keyboard ten times. After step 7 there is no file in a repo to refuse, and the
largest open hole in `docs/MULTIPLEXER-READINESS.md` closes by construction rather than by a guard.

---

## 6. What it does not solve, and what it costs

Not sold. In order of how likely each is to be mistaken for something this design fixed.

1. **At one uid, nothing is isolated — and the keyring is slightly worse before it is better.** Every
   same-uid process can read `<channel home>/conversations/*/secret` with one `ls`, where today it
   must find the repos first. 0700 + 0600 is exactly the protection the repo token already had, no
   more. The only real boundary here is a container bind-mount, and that boundary belongs to the
   dispatcher, not to the hub. Anyone reading this as "secrets are now safe" has misread it: it makes
   them **absent from git**, which is a different and smaller claim.
2. **The book runs out at a keyboard.** A seed that wants a ninth room at 3am is blocked until he
   types. That is the admission line doing exactly what it is for. The refill-from-a-tap door is
   legal under the existing rule and is deliberately **not** taken here — see §7 question 3.
3. **A leaked grant directory plus the keyring is N rooms.** Bounded, cannot reach another
   conversation, cannot enrol, cannot flip `enabled` — but real, and a shape of loss a single repo
   token did not have.
4. **The title file is a real widening**, from "titles come only from a terminal" to "titles come
   from whoever holds the slot". Bounded to a display string, shape-refused, read once. Named in §2,
   and question 2 in §7 is the operator's chance to refuse it.
5. **Lane uniqueness is untouched (TAXONOMY #2).** `bind_topic` is still a bare insert
   (`registry.rs:345`), `lane_is_addressable` is still shape-only (`hub.rs:773`), nothing prunes
   `lane_topics` (`registry.rs:88` says so), and git dedupes worktree names only among live worktrees.
   A recycled name still inherits the old conversation's topic. Conversation ids get a real
   reservation primitive for free — `mkdir` on a random id is atomic — and lanes get nothing.
6. **The phone still says "worktree" (TAXONOMY #3) and the title still clips from the left (#4).**
   Rooms escape both by being top-level, but the strings stay wrong for any dispatcher-supplied lane
   name and the clip at `registry.rs:110` is still calibrated on machine names.
7. **The availability prize is NOT collected.** `read_projects` failing still means no project on the
   box can connect (`registry.rs:283-296` logs exactly that). **"A conversation is a directory"** wins
   that by splitting the store per conversation; this design keeps `projects.json` as the single store
   of hashes precisely so `resolve` needs no second source, and that trade is deliberate. The
   directory layout is grafted; the store split is not.
8. **No index, on purpose.** `addr_for_topic` (`hub.rs:1264`) is still a linear scan over projects ×
   lane topics, run on every message he types. **"A conversation is a directory"** offers
   `index/by-topic/<id>` and the rule that makes it safe — record is truth, index is a cache any
   process may rebuild, a miss scans, a scan repairs, a disagreement logs and loses. Worth taking
   later, on its own, with that rule attached; not here, because a second routing source in a slice
   that already moves credentials is how this repo shipped its registry-drift bug once already.
9. **The container story is incomplete, and identity is not what breaks first in it.** Every adapter
   derives the socket from its **own** `getuid()` — one derivation now, in `attach.ts:119` and
   `attach.ts:338-339`, which is the point of the one-namespace slice — and that is 0
   under `--unshare-user`; the dispatcher must bind-mount the socket and set `KICKOFF_HUB_SOCKET`, and
   `KICKOFF_HUB_RELAY_DIR` must move inside too (`docs/ATTACHING.md` §10 works the container through). From **"The door is the credential"**, and it is a
   production requirement for any container work regardless of which design wins: **bind-mount the
   DIRECTORY holding a socket, never the socket file.** `bind` unlinks and rebinds on every start
   (`hub.rs:816`), so a mounted socket file pins a dead inode and the container gets ECONNREFUSED for
   ever after the first hub restart. The latent `NonZeroI32` peercred note (`hub.rs:2313`) is
   untouched and unreachable while the hub is on the host.
10. **Seam ④ is untouched.** Who starts a container is still open, and this design neither adopts nor
    refuses the launcher-as-adapter proposal.
11. **`~/.claude/channels/` is not adopted literally.** Measured: it exists, it is **empty**, and it is
    `drwxr-xr-x`. The pattern is followed — a space the channel owns, outside every repo, keyed by
    channel, laid out as the channel sees fit — at an XDG-correct location, with
    `HERDR_TG_CHANNEL_HOME` so the box can point at `~/.claude/channels/herdr-tg` if he meant the
    path. Two reasons, the second more honest than the first: `~/.claude` is the Claude **engine's**
    directory and opencode has none, so a channel serving two engines should not live inside one of
    them; and if he did mean it literally, that is a config line rather than a redesign, so choosing
    wrong costs nothing.
12. **`asks.json`, the audit log and the heartbeat stay global.** The per-conversation directory is
    the seam that would let them split later; nothing moves now.

### The cost

**Rust.** `registry.rs` (936 lines) takes the bulk: the keyring writer, the `by-repo` link, blank-row
minting with a random `c-` id, the `vacant` field, and the containment assert. `lock.rs` gains
`channel_home()` and the 0700 fix at both bare `create_dir_all` sites. `cmd/enroll.rs` (663 lines)
grows `open`, `grant` and `adopt-secrets` beside `enrol` and eventually loses the ~130-line git
guard — **net negative**. `main.rs` gains three clap verbs. `bot.rs` gains one predicate.
`hub.rs` is touched in **one** place: the title read in `topic_for` (`hub.rs:1345-1348`) and the
matching composition at `hub.rs:1664-1672`. `admit`, `Addr`, the claim, the ledger and the tap route
are not touched at all. `crates/hub-proto/`: **zero**. `crates/herdr-client/`: **zero**.

**TypeScript.** `where.ts` (168 lines): the four-term ladder replaces `findProject`/`searchUpward`,
`:143` deletes, `faninSocket` re-keys — roughly net neutral. `server.ts` (778): the ladder, three
refusal sentences, the fan-in address. `bridge.ts` (529): the same ladder a second time in a second
process. `fanin.ts`: the listen address.

**Tests.** The 29 `hub.token` references across nine files; `hub/tests.rs:1916` rewritten from "a lane
worktree crosses to the main tree" to "a lane worktree and its main tree read one credential by
construction".

**Docs.** `INTERFACES.md` (two nouns), `TAXONOMY.md`, `MULTIPLEXER-READINESS.md`, and
`CAPABILITIES.md` REQUIRES #1, which changes from "enrolment, at a terminal, **per repo**" to "per
conversation". That is a **promise** change, and that file's own rule says kickoff must be told rather
than left to diff it.

### The riskiest parts, in order

1. **Step 0 landing late.** If any secret reaches the keyring before the 0700 assert, every new box
   ships a world-traversable parent over a directory of credentials. Measured, not feared.
2. **A conv-id becomes a path segment.** Shape-refuse at the door on `^[pc]-[0-9a-f]{12}$`, **and** a
   canonicalised containment assert immediately before every write — because the check and the write
   live in different functions and this repo has already shipped a defect of exactly that form. The
   graft is **"The conversation record"**'s, and it is the treatment `lane_is_addressable`
   (`hub.rs:773`) already gives a lane name.
3. **The RED test to write first**, from **"A conversation is a directory"**: *a secret minted for
   conversation A can never resolve to conversation B.* There is no two-source `resolve` here to get
   wrong, which is the point — but the test is what proves that claim rather than asserting it, and
   the only failure that matters in this area is the silent one.
4. **The claim race.** Two dispatchers racing for one slot is caught by the `O_EXCL` rename; if it
   were not, the one-claim rule catches it as `already_claimed` (`hub.rs:1127`) — a lockout,
   never cross-talk, because a tap is refused when the record's instance does not match the claim's.

---

## 7. The open questions — only the operator can answer these

1. **Where does the channel's space live?** XDG-correct at `$XDG_STATE_HOME/herdr-tg`, or literally
   `~/.claude/channels/herdr-tg` because that is the pattern he named? Measured: that directory
   exists and is empty, so there is nothing to interoperate with and no schema to copy. One config
   line either way.
2. **May the dispatcher name a room?** Yes → the `title` file, and a room reaches his phone named
   after its function the first time it speaks, at the cost of the widening in §2. No → the machine
   names it "room 3 of &lt;seed&gt;" and he renames the topic once on his phone, which sticks for ever
   because the hub never renames a topic. This is a values question about who may write a string he
   reads, not a technical one.
3. **How big is a book, and may a tap refill it?** N is his number. Refilling from his phone is
   *legal* under the rule as stated — a launcher adapter holding a terminal-granted refill capability
   would be seam ④ of `docs/INTERFACES.md`, and it would work. It is deliberately not taken here,
   because whether a phone may create credentials at all is his values question and this design is
   worth more if it does not need the answer.
4. **Does a room outlive its session, and must its topic survive with nothing connected?** This is
   `docs/CAPABILITIES.md` OPEN #2 and it decides whether a claimed slot is ever returned to the book.
   Today nothing returns and nothing prunes; that is the shape this design assumes.
5. **Should the operator-facing word stop being "worktree"?** TAXONOMY #3. Rooms escape it by being
   top-level, so this design does not force the answer — but a dispatcher-named lane makes those three
   strings false, and neutral wording costs nothing behaviourally while dropping a fact he has today.
6. **Delete the repo tokens fleet-wide, or per project at his pace?** Step 7 is per-project and
   reversible until it runs. Doing all three at once collects the prize a day earlier and gives up the
   ability to roll one back independently.

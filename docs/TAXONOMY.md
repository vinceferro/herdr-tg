<!-- TAXONOMY, 3 September 2026. Records the operator's own words about what a seed, a room and a
     lane are, and holds them against what is built. His words are the authority; everything below
     them is subordinate to them. This is an ANALYSIS: no production code was changed for it, and
     every claim traces to a file:line or to something that was run. -->

# Seed, room, lane

## His words, verbatim

> The idea is that with kickoff you can start a project, and as an operator you sit in the steering
> room for the idea and vision brainstorming. This is a kickoff seed. One opencode|claude instance
> -> 1 herdr-topic.
>
> As the convo grows, the main chat proposes a new function for the AI business -> this translates
> into a room -> a new kickoff process (opencode | claude) -> a new herdr-topic.
>
> The lanes that kickoff refers to are a way to mimic claude code ultracode, so the chat thread
> doesn't stall.
>
> All of those instances will be either docker or bwrap, so we don't need to ask for permissions.

## The three kinds

| | what it is | who starts it | what it is FOR | does he sit in it |
| --- | --- | --- | --- | --- |
| **seed** | the steering room — one engine instance, one topic | the operator, at a terminal | idea and vision brainstorming | **yes** |
| **room** | a function of the business — one engine instance, one topic | the seed's conversation proposes it; kickoff spawns it | doing one function, with its own charter and memory scope | **yes** |
| **lane** | background parallelism inside a conversation | kickoff dispatches it | keeping the chat thread from stalling while work runs | **no** |

Three things follow that this project had not absorbed before today.

**A room is a SIBLING of the seed, not a child of it.** It is born *from* the seed's conversation,
which is a fact about how it came to exist, not about who owns it. Its shape is the seed's shape.

**A lane is not a conversation he sits in.** It has the same shape here today, but a different
purpose. Whatever we do about rooms, a lane stays what it is.

**Permissions are granted before the process starts.** Half of the opencode adapter's ask traffic
is the permission prompt relay, and a sandbox with permissions already granted stops that traffic
at the source.

## What the hub already expresses

**The address is opaque to the hub, and the secret proves only the project.** That one sentence is
why this taxonomy is a naming question rather than a rebuild.

`Addr` is `(ProjectId, Option<LaneId>)` and nothing more (`crates/herdr-tg/src/hub.rs:719`).
`LaneId` is an opaque string that is "never parsed here, and never resolved to anything"
(`crates/hub-proto/src/ids.rs:79`). The only check on it is shape — non-empty, at most 64 bytes,
no `.` or `..`, no separator, no control character (`crates/herdr-tg/src/hub.rs:773`) — and every
clause of that check is justified by audit-forging, never by meaning. The project half never comes
from the wire, so a bridge naming an address can only ever reach the repo it has already proved it
holds the secret for.

Every decision downstream keys on that pair and reads nothing out of it: the claim
(`hub.rs:1127`), the topic (`hub.rs:1340`), the ledger, the tap route.

So **the hub needs no change to address a room.** Nothing in `hub.rs`, `registry.rs` or `hub-proto`
would notice that an address string came from a config file rather than from git. This was written
down as a contract the same day, in `docs/CAPABILITIES.md` — the hub allocates nothing, and
uniqueness within a project belongs to whoever dispatches.

What the hub cannot express is a **kind**. `hello` carries project, token, instance, repo, pid and
an optional address; `welcome` echoes project, address, topic and limits. There is no field that
says which of the three kinds a connection is, and every renderer treats them identically.

## What breaks, ranked

Each row is a defect an analyst proved and a sceptic failed to refute. Rank is by what bites first,
not by severity.

| # | what breaks | where |
| --- | --- | --- |
| 1 | A room inside an enrolled repo **cannot get a second address at all** — it arrives as the seed and is refused | `where.ts:86`, `registry.rs:412`, `hub.rs:1127` |
| 2 | Nothing checks address uniqueness; the guarantee is imported from git being the only producer, and git only dedupes among **live** worktrees | `hub.rs:773`, `registry.rs:336`, `registry.rs:345` |
| 3 | Three operator-facing strings and three contract doc comments assert an address **is a git worktree** | `hub.rs:1427`, `bot.rs:500`, `bot.rs:521`, `enroll.rs:135`, `ids.rs:79`, `frame.rs:185` |
| 4 | The topic title clips the address from the **left**, calibrated on names that differ in their tails | `registry.rs:98`, `registry.rs:110` |
| 5 | The fan-in socket is keyed on the git **repository**, not the enrolled project — two projects in one repo collide on the opencode path | `where.ts:115`, `fanin.ts:74` |
| 6 | In a lane worktree the enrol hint names the **worktree**, and following it mints a second project | `server.ts:493` |
| 7 | Of the three signals he says are still worth carrying, one is dropped and two are not carried | `bridge.ts:472`, `hub.rs:2078` |
| 8 | Every socket-boundary failure reports itself as "the hub is not listening" | `bridge.ts:244`, `hub-link.ts:454` |

### 1. A room inside an enrolled repo cannot be addressed

This is the constraint that bites first, and it needs no name collision to reach.

The address is a function of the **launch directory**, not of a name anyone picks. `factsFor` sets a
lane only when `projectTop !== mainTop` — that is, only inside a linked git worktree
(`plugins/kickoff-channel/where.ts:86`). A plain subdirectory of the org repo gets `lane: null`, and
`hello` then omits the key entirely (`server.ts:555`). `findProject` walks up to the nearest
`.kickoff/hub.token`, which is the org root's. So every room presents the identical `(secret, no
address)` pair: one claim, one topic, and the second room to start is refused `already_claimed`
against the seed (`hub.rs:1127`).

Nor can it be its own project instead: `enrol` refuses any folder strictly below an already-enrolled
one, saying it "would split one project across two chats"
(`crates/herdr-tg/src/registry.rs:412`). And enrolment is terminal-only by design
(`docs/INTERFACES.md`), so nothing kickoff spawns can mint a room's identity for itself.

There is no env var, file or argument that can supply an address today. Git is the only producer.
Three rooms in one org repo, as plain directories, is not three conversations — it is one
conversation and two lockouts.

**A room that is its own top-level repo works today with zero changes**, gets its own secret, its own
project, its own topic, and never touches the lane namespace. That is also the shape his taxonomy
describes: same shape as the seed, sibling not child.

### 2. Uniqueness is imported, not checked

The hub has no uniqueness check anywhere. `lane_is_addressable` is shape-only; `bind_topic` is a
bare insert (`registry.rs:345`); `topic_of` a bare get (`registry.rs:336`). Collision-freedom rests
entirely on one line of TypeScript — git's worktree name, which git guarantees unique.

That guarantee is a property of git being the **only** producer, and it is narrower than it sounds:
**git dedupes among currently-registered worktrees, never over time.** Measured on git 2.55.0 in a
scratch repo: `worktree add ../wip` takes the name `wip`; after `worktree remove` and `prune`, a
fresh `worktree add ../second/wip` takes `wip` again. Nothing prunes `lane_topics`
(`registry.rs:88`, which says so in as many words), so the recycled name inherits the old
conversation's topic and its whole rolling history.

`docs/INTERFACES.md` says the worktree name is "unique across a repository". That is true
concurrently and false across a day. It has not bitten, because kickoff's own dispatched lane names
carry a timestamp.

Harm is bounded, and worth stating so nobody over-reacts. A collision costs a **lockout**, a shared
topic, and a false "the session that asked this restarted" sweep. It never costs cross-talk: a tap
is refused when the record's instance does not match the claim's, so an answer cannot enter the
wrong agent's turn.

### 3. The phone says "worktree"

Three strings the operator reads, and they would be false the first day a room is addressed as a
lane:

* `hub.rs:1427` — the greeting, and therefore the **first message in a brand-new topic**:
  *"{lane} is connected — a separate worktree of {project_title}."*
* `bot.rs:521` — `/projects`: *"↳ {} — a worktree of it"*, indented under its project.
* `bot.rs:500` — the parent row: *"not connected itself — only its worktrees are"*.

Plus `cmd/enroll.rs:135` in the terminal listing, and the wire contract's own doc comments —
`crates/hub-proto/src/ids.rs:79` opens *"One worktree of a project"*, in the crate `CLAUDE.md` says
"knows nothing about herdr, kickoff, claude or panes, and must not learn".

`docs/INTERFACES.md` lists *"Knowing what a lane, a proof, a worktree, or a re-ground is"* under
things explicitly NOT capabilities of the hub. These strings assert about the address a fact the hub
is documented not to know and structurally cannot verify. They are accurate today only because no
shipped adapter can mint a non-worktree address.

Note the nesting is not just wording: grouping an address under its project is capability 2 of the
closed list, and a test pins the indentation as a property.

### 4. The title clip assumes the name differs in its tail

`lane_title` shows at most 14 characters of the address and clips from the **left**
(`registry.rs:110`), because dispatched lane names share a `lane-<date>-` head and differ only in
their tails. A function name inverts that: it is distinguished by its head. Two rooms sharing a tail
render as one title.

Two corrections that keep this in proportion. It is **not** taxonomy-induced — two descriptive
worktree names ending in the same word collide today the same way. And the severity is a degraded
forum-list row only: the topics are distinct, delivery is unaffected, the greeting names the address
in full, and `/projects` lists every live address unclipped. Do not reach for a middle-clip: it
trades this collision class for another one the current code tells apart.

### 5–8, in brief

**5.** `faninSocket` hashes `(mainTop, lane)` (`where.ts:115`) — the git repository, not the
resolved project. Two enrolled projects inside one repo derive one relay address. It fails closed
(the relay re-checks the secret and turns the stranger away), but the second project's opencode path
is dead, and both refusal sentences are false. Reachable today with an ordinary monorepo, no rooms
involved. The three enrolled projects on this box are separate repos, so nothing is misrouted now.

**6.** `const dir = PROJECT_TOP ?? project?.repo ?? null` (`server.ts:493`). In a lane worktree
`PROJECT_TOP` is the worktree while the secret was found in the repo, so the hint says
`herdr-tg enroll <the worktree>`. That command succeeds — a worktree is a sibling of the repo, not a
child, so the containment guard cannot fire — and mints a second project, moving a live conversation
somewhere new on the phone. The function's own docstring forbids exactly this. One-line fix, both
operands already in hand; not applied here, and it is owed a RED test because nothing pins the
current order.

**7.** His residual list is *"a session erroring, a turn ending, a lane going quiet"*. A turn ending
reaches the phone only when the agent **chooses** to call `done` through the shared tool server; the
bridge's own mapping of `session.idle` is `beat` (`bridge.ts:472`), which the hub acks and drops
(`hub.rs:2078`). A session erroring has no case at all. A lane going quiet has no opencode event
behind it and needs a timer in the adapter. Note there is no quiet third option to reach for: `Say`
and `Done` are the same call (`hub.rs:1929`), so the choice is a message that notifies, or nothing.

**8.** Both catches discard the error object (`bridge.ts:244`, `hub-link.ts:454`), so a missing
bind-mount, a wrong path and a permission refusal all print one sentence naming a service that is
healthy. On the Claude side the sentence that actually reaches the agent — and therefore his phone —
is stronger and worse: *"The hub is not running"*, a diagnosis rather than a symptom.

## The open decisions — his to make

Stated as questions, because each turns on what he wants rather than on what the code can do.

1. **Where does a room live?** Its own top-level repo, or a directory inside the org repo?
   Its own repo works today with no change to anything, and matches "a room is a sibling of the
   seed". Inside the org repo means a room must be an address under the project, which is the
   subordinate shape, and makes every question below live.

2. **If a room's name does not come from git, who mints it and who reserves it?** git dedupes only
   within `.git/worktrees/` and a config file dedupes only within itself; neither can see the other,
   and the hub will not dedupe for either. Whoever mints must also reserve.

3. **Should the operator-facing word stop being "worktree"?** Neutral wording costs nothing
   behaviourally and removes a claim the hub cannot back — but it drops a fact he has today, that a
   row is a checkout.

4. **Should a turn that ends without the agent saying so reach his phone at all?** At fourteen
   projects this is the difference between a forum he reads and a forum he mutes, and today the only
   two options are a message that notifies or silence.

5. **Does a room outlive its session; can two rooms of one org be live at once; must a room's topic
   survive with nothing connected?** These three are kickoff's to answer, and they are already on
   the wire to them. They decide whether one address space is enough.

## What containers change

His last sentence — *"All of those instances will be either docker or bwrap, so we don't need to ask
for permissions"* — puts the **engine instances** in containers. Nothing proposes containerising the
hub, and the distinction matters below.

**The permission relay goes quiet, but the code does not go away.** Permission-exclusive code is 40
of `bridge.ts`'s 529 lines. It is a small fraction of the text and half of the adapter's purpose:
one of its two ask sources. A container does not delete those lines — opencode emits
`permission.v2.asked` because of how opencode is **configured**. The switch is one line, and it is
already set: `~/scratch/oc-dogfood/opencode.json` carries `"permission": {"bash": "ask", "edit":
"ask"}`. Flipping it to `"allow"` tests the premise today, with no container. Do that before
deleting anything.

**What survives a container, measured.** `SO_PEERCRED` is read in the **hub's** namespace, so a
container's opinion of its own identity never reaches the hub. Reproduced here: a client under
`bwrap --unshare-user --uid 0 --gid 0 --unshare-pid`, which sees itself as `uid=0 pid=2`, was read by
the listener as `uid=1001 pid=1051393`, with a live `/proc` entry. So gate 1 (`hub.rs:916`) admits
correctly rather than by accident, and the pid the single-claim rule and the orphan sweep depend on
(`hub.rs:788`) stays a real host fact under `--unshare-pid`.

**What was NOT tested, stated plainly.** A peer whose kuid maps to something other than the hub's
own — rootful docker at kuid 0 — was not produced. When a differently-mapped peer *was* constructed,
it failed at `connect()` with EACCES against the socket's real mode 0600 and never reached gate 1 at
all; it only reached gate 1 with the socket deliberately loosened. So the ordering is: mode 0600
stops it first, and gate 1 is the second layer, which is what `hub.rs`'s own module doc says. Do not
claim gate 1 is the only thing standing there.

**What actually breaks first in a container, and it is not a gate.** Both bridges derive the socket
path from **their own** uid — `/run/user/${process.getuid()}/kickoff/hub.sock`
(`where.ts:96`, `bridge.ts:49`). In the namespace above, `getuid()` returns 0, so the bridge looks
for `/run/user/0/kickoff/hub.sock` while the hub listens on the host user's. `KICKOFF_HUB_SOCKET`
overrides it, but `where.ts` calls the path "derived and never configured" and every existing use of
that variable in the tree is a test. Ordered: wrong path first, then permissions, then gate 1.

**One latent soundness note, unreachable as things stand.** `rustix::net::sockopt::socket_peercred`
(`hub.rs:2313`) reads into a `UCred` whose `pid` is a `NonZeroI32`. The kernel writes 0 whenever the
**reading** process's pid namespace is not an ancestor of the peer's, which the compiler's niche
layout turns into an `Err` — so it refuses the connection, but by way of undefined behaviour and
with a message that can read "Success" or "No such file or directory". A hub on the host is always
an ancestor of every container's pid namespace, so this needs the **hub itself** containerised
below or beside its adapters. Two warnings if it is ever fixed: the obvious swap to tokio's
`peer_cred()` makes it **worse**, because tokio sets `Some(pid)` unconditionally on Linux and a
guard keyed on `None` would never fire, letting pid 0 reach the eviction rule and every incumbent
look like a corpse. Key any guard on the value 0, not on the `Option`.

## Where this leaves the question

*Does one opaque address string per conversation serve seed, room and lane?*

**For routing, yes, and provably** — the hub reads nothing out of the string, so a room is
addressable the moment something can name one.

**For naming, not yet** — nothing can supply a name that did not come from git, and the seed's own
address is the one a room would land on by default.

**For the operator's eye, no** — a room addressed this way is titled by a clip built for machine
names, rendered as a child of the seed, and described in a git word for a thing that does not exist.

The routing answer is the hub's to give and it has given it. The other two follow from one decision
that is not the hub's: **where a room lives.**

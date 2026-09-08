<!-- PROPOSAL, 2026-09-01. Written after the operator asked for interfaces and capabilities
     rather than another map of what is currently running. Nothing here is built beyond
     what is marked BUILT. -->

# The seams

Four systems meet here. Only one seam between them is defined today, and the value of this
document is naming the other three before anything is written against them.

    operator's phone
          │  Telegram
    ┌─────┴──────┐
    │  the hub   │  identity · presence · delivery · resolution · alarm
    └─────┬──────┘
          │  ① hub-proto — the frames             ← BUILT
          │     over a transport: AF_UNIX, admitted on the peer's uid
    ┌─────┴──────┐
    │  adapter   │  one per engine
    └─────┬──────┘
          │  ② engine API — MCP channel, or HTTP + events
    ┌─────┴──────┐
    │   engine   │  Claude Code · opencode
    └─────┬──────┘
          │  ③ the discipline — re-ground, lanes, worktrees, proofs
    ┌─────┴──────┐
    │  kickoff   │
    └────────────┘

## The governing idea

**The hub's vocabulary is conversation, never orchestration.**

It knows about things that can *say*, *ask*, and *end*. It does not know what a lane is, what a
proof is, what re-grounding means, or that kickoff exists. Every one of those is expressed
**through** the conversation primitives by an adapter — which is why the hub can serve two engines
that have nothing in common, and why a third would need no change to it.

The moment the hub learns the word "lane", it stops being a multiplexer and becomes kickoff's
second implementation.

## Seam ① — hub ↔ adapter. BUILT.

`crates/hub-proto`. NDJSON, nine frames up, six down, over `AF_UNIX`. Zero mentions of herdr,
kickoff, claude, tmux or panes in `hub.rs`.

### The transport is a seam inside seam ①, not a fifth seam

Two things were being read as one. **The frames** say what a conversation is: say, ask, resolve, end,
ack. **The transport** says who may open a connection at all, what the hub can prove about whoever
did, and — because bytes cross by mount and never on the wire — which offers that connection can
have. Naming the difference is what lets the next question be answered without reopening the first:
a bridge somewhere else changes the transport and not one frame.

There is one transport today and it is local: `AF_UNIX` at `/run/user/<uid>/kickoff/hub.sock`, a
`0700` directory and a `0600` socket, admitted on the **peer's uid** taken off the socket by the
kernel **and** the secret in `hello`, both. What the transport supplies beyond that is a pid, which
is the fence for a bridge that dies without a `bye` (`/proc/<pid>`), and a shared filesystem, which
is the whole of files — a path is worth nothing to a peer that cannot open it, so a connection that
does not share the hub's filesystem is offered no outbox. `docs/CAPABILITIES.md` REQUIRES 3 is the
sentence another org builds against; offer 9 is where files belong to the transport rather than to
the wire.

Numbering stays at four seams, deliberately: `docs/ATTACHING.md` §11 and `docs/CAPABILITIES.md`
OPEN 1 both cite these numbers, and a transport is not a fourth party meeting the other three — it is
the floor seam ① stands on. Where the hub keeps the difference is `transport.rs` beside `hub.rs`: one
knows what a socket and a peer are and nothing about a claim, the other knows what a claim is and
nothing about a socket.

**A transport that is not this machine is OPEN, not refused** — `docs/CAPABILITIES.md` OPEN 4, with
our proposed shape written down in advance: a gateway on the hub's box holding an ordinary local
connection for a peer elsewhere, and a second kind of connection identity inside the hub beside
today's local peer. Every frame would mean exactly what it means now; what a remote peer could not
have is the uid, the `/proc` fence and the outbox. Nothing of it is built, and building it changes
no frame.

The contract, in one line each:

* **Identity is a secret, never a name.** `hello` carries no display name; the hub resolves the
  secret to a project and takes the title from its own registry.
* **A name is an id, never a path. BUILT, 8 September.** The `welcome` says which conversation the
  secret resolved to and which project that conversation belongs to, both as opaque ids, and the
  read-only inventory relates a room to its project by `seed` — so two programs on two boxes join
  their records on the same strings. A path answers *where* and no question a fleet asks: a room and
  its seed share one, a wall mounts a checkout somewhere else, and a move makes a new one for the
  same work. `docs/ATTACHING.md` §3b is the table of which things an adapter handles are fleet
  identity and which are one machine's business; `hello`'s `repo` and `pid` are in the second half,
  which is why both became optional in the same change.
* **Authority flows one way.** No frame carries addressing. The hub knows which connection is which.
* **Every frame is acked exactly once**, with three delivery values — `yes`, `no`, `unseen` —
  because a send that timed out may or may not have landed and there is no way to ask.
* **A tap resolves against a written record**, never a button's position — and, since
  7 September, an adapter that promised to (`confirms: ["choice"]`) answers for what became of it,
  so what the operator reads is what happened in the agent's turn rather than what happened to a
  frame.
* **A run holds a lease, and it is the hub's to mint.** The `instance` says which PROCESS is
  speaking and is what a tap and a retirement are matched on; the lease — a number on the
  `welcome`'s own envelope, stamped back onto everything the bridge says — says which RUN of
  the address holds it now. A run a later one replaced is refused rather than quietly
  overwritten, which is what happened to a dead bridge's claim before: it went on draining
  what it had queued into a conversation its successor already owned.
* **Skew is normal**: unknown frame kind is ignored, unknown field is ignored, major version
  mismatch is refused.

**The one widening this seam needed. BUILT, 2 September.** `hello` gained an optional `lane`, and
the claim key is now `(project, lane)`. The secret still proves only the PROJECT — a lane and its
project are one repo and one trust domain, so a bridge naming a lane can only ever affect its own.

Additive in both directions, and both directions matter because a channel plugin restarts only when
its session does: a `hello` with no lane is byte for byte what it always was and is still the
project's own voice, and a `hello` that names one parses on a build that has never heard of the word.
A lane the hub will not address — empty, over-long, or carrying a control character that would forge
a line in the audit — is refused with `bad_lane`, permanently rather than as something to retry.

**Where the address comes from, since 4 September: whoever dispatched, and git only as a fallback.**
`KICKOFF_HUB_ADDRESS` is the dispatcher's word and goes on the wire verbatim; `docs/ATTACHING.md` is
the contract, and it is the same contract for all three adapters because they share one reader.
An adapter checks the address against the hub's own shape rules BEFORE it dials, because `bad_lane`
is permanent and learning it from a refusal costs a claim and a round trip to be told something that
was readable off the configuration.

With nobody dispatching, an adapter learns its lane from **git's own name for the worktree** — the
last segment of `--git-dir`, which git guarantees unique across a repository — crossing to the main
worktree via `--git-common-dir` to find the secret: a lane worktree has no `.kickoff/hub.token` in it, because
the secret is gitignored and never checked out into one. Not the checkout's folder name, which git
does not dedupe: `~/a/wip` and `~/b/wip` are two trees that would have presented one address.

The echo closes the other direction. `welcome` carries the lane the hub admitted, and a bridge that
named one and does not get it back refuses rather than connecting — because an old hub ignores the
unknown field and would admit the worktree AS THE PROJECT, taking its claim and its topic while the
project's own session is turned away. A channel plugin restarts only when its session does, so
new-bridge/old-hub is the ordinary middle of an upgrade rather than an exotic state.

**The second widening, and it is the fence the first one needed. BUILT, 7 September.** `hello`
gained an optional `confirms` and the envelope an optional `generation`, both omitted when
absent, so a bridge from before either is byte for byte what it always was. The generation is
the lease above; `confirms: ["choice"]` is a bridge promising to say what became of the
operator's tap, and the hub holds his receipt open only for a bridge that promised. Additive
in both directions again, and for the same reason: a channel plugin restarts only when its
session does, so new-bridge/old-hub and old-bridge/new-hub are both the ordinary middle of an
upgrade. `docs/ATTACHING.md` §6 is the contract for both.

The opencode adapter can now be TOLD its address, like any other — one bridge per conversation,
named by whoever started it. What it still cannot do is serve SEVERAL conversations from one event
stream: that needs `/api/event`, whose payload carries `location.directory` per event, and without
that fact there is nothing to route on. Told nothing, it speaks for the project and attaches to the
project's own relay, exactly as before.

## Seam ② — adapter ↔ engine. Engine-specific by definition.

This is where the two engines stop resembling each other, and that is fine: it is the only place
they are allowed to.

| | Claude Code | opencode |
|---|---|---|
| attachment | MCP channel plugin, in-process | HTTP + `/event` stream, out-of-process |
| lifetime | dies with the session | outlives sessions |
| granularity | one session | one server, many sessions |
| status | BUILT, proven against the real hub | BUILT, `adapters/kickoff-hub-attach/ --opencode` |

Consequence worth stating plainly: the opencode adapter is **one bridge per project, not per
lane**, holding one hub connection per lane.

### The fan-in. BUILT, 3 September — and it is on the ADAPTER side, on purpose.

An opencode agent now has the same deliberate tools a Claude agent has: the very same
`plugins/kickoff-channel/server.ts`, declared under opencode's `mcp` key as a stdio MCP server. The
operator asked for exactly that — *"I'd rather have a channel like the one for claude code so the
agent actually invokes sending a telegram message"* — having used a plugin that streamed everything
and disliked it.

That makes TWO things want to speak for one addressable thing: the tool server, carrying what the
agent CHOSE to say, and the event bridge, carrying the permission and question prompts it did not
choose. The hub admits one live claim per address and refuses the second with `already_claimed`.

**The joining belongs to the adapter, and the hub does not change at all.** One process per
addressable thing holds the connection and opens a door; the producers talk to it over a local socket
that speaks hub-proto unchanged. The hub sees one `hello`, one pid, one claim — so this is not a
seventh capability, and the closed list below is still six. That process is
`adapters/kickoff-hub-attach/` (the door was `adapters/fanin/`; both it and the opencode event
bridge folded into the one command — see `docs/ATTACHING.md` §13).

The relay is not a pipe: it answers `hello` with the `welcome` it holds (lane echo included),
rewrites envelope ids, namespaces `ask_id` so a tap on one agent's question can never be delivered
to the other, answers the hub's `ping` itself so a wedged producer cannot cost the lane its claim,
and shares the queue out among however many producers are attached.

**Standing in front of the hub means taking on the hub's lifecycle job.** The hub retires a dead
asker's questions from two facts it reads off the CLAIM — the `instance` in `hello`, and the pid
holding the socket — and behind a relay both are the relay's, for every producer, for ever. So
neither sweep can see a producer die, and the operator would keep a keyboard for a question whose
agent is gone. The relay therefore knows a producer by the `instance` in its own `hello` rather than
by its socket, withdraws the open questions of one that does not come home within a grace period,
and writes its own instance and its open questions down beside its socket so that restarting IT
comes back as the same voice rather than voiding what its producers are still waiting on.

The direct path was never exposed to any of that, and the earlier draft of this paragraph was wrong
to say it was: a channel plugin mints one `instance` for the life of its session and the hub reloads
its ledger from disk, so restarting `herdr-tg` under a live Claude session leaves that session's
questions answerable — its arrival sweep skips its own instance by construction, and a tap resolves
because the instance on the record still matches the claim.

The sentences an agent reads differ between the engines wherever the ENGINE decides whether they are
true, and nowhere else. The operator's
answer comes back as an MCP notification that Claude Code injects into the agent's turn and that
opencode has no passthrough for — measured, from both real clients, neither of which advertises any
capability about channels. So `ask` promises an answer is coming only to a client that can be handed
one, and tells the others to carry on without one. Promising it everywhere would be the incident the
whole three-outcome vocabulary was written for, one engine further out.

The same notification carries every correction — the hub saying a frame it took never reached his
phone — so it decides one more thing: whether a "he was reached" sentence can still be taken back.
Where it cannot, that sentence is the last word there will ever be, and each of the four says so, in
words the `instructions` block teaches the agent to look for. The queued and permanent sentences are
byte for byte the same on both engines, because those two are already final.

What the first slice found, 2 September: opencode publishes `question.v2.asked` with real
`options[{label, description}]` and `permission.v2.asked` with a closed reply set of
once/always/reject. So the adapter maps a question to an `ask` and a tap to a reply without
parsing anything — the structure herdr threw away is on the wire here. Its `/session` does report a
`directory` per session, and the adapter deliberately does **not** use it to serve several projects
from one server: identity is a secret the hub resolves, and one bridge per project addressed by URL
is also what survives each agent moving into its own container. The Claude adapter is one per session because that is
all a channel plugin can be.

## Seam ③ — the discipline. Undefined, and it belongs to the ADAPTER.

kickoff's discipline is re-grounding, lanes, worktrees and machine-derived proofs. None of it is
the hub's business. All of it reaches the operator through seam ①:

| kickoff thing | how it reaches the phone | hub change |
|---|---|---|
| a lane finished, proof passed | `done{text}` | none |
| a lane finished, proof FAILED | `done{text}` saying so | none |
| a session is stale (headroom high) | `ask{"reground?", [yes, not now]}` | none |
| a lane needs a decision | it should fail its proof instead | none |

**Re-grounding is the shape of the whole idea.** The adapter notices the session is stale, posts an
`ask`, the operator taps, and the adapter touches `.kickoff/refresh-requested` — which kickoff's own
header calls *"the single mechanism the supervisor watches"*. The hub never learns what re-grounding
is. It carried a question and returned an answer, which is all it does.

That answers "does the hub need a lifecycle capability?" — **no**, for everything except starting
something that does not exist yet.

## Seam ④ — starting what is not running. Undefined.

An `ask` cannot come from a session that has not started. This is the one case the conversation
primitives genuinely cannot express, and it is worth solving without widening the hub.

**Proposal: the launcher is just another adapter.** A long-lived process, enrolled like any project,
holding a hub connection and a topic of its own. It offers what it can start as an `ask` with one
button per enrolled project. A tap comes back as a `choice`, and the launcher — not the hub — acts.

What that buys:

* The hub still spawns nothing. Zero `Command` in the binary, and no string from the wire to a
  command line.
* Starting is a `choice` against options the launcher minted, so inbound content still SELECTS from
  what the machine knows and never NAMES.
* No new frame kind, no new hub capability, no new trust boundary. A launcher is a bridge that
  happens to start things.
* It is the natural home for `systemctl --user start kickoff@<project>`, if that is the mechanism —
  and the launcher can be replaced without the hub noticing.

## Capabilities of this project — the closed list

The hub does exactly these things, and adding a seventh is a decision, not a refactor.

1. **Identity** — resolve a secret to a project; one live claim per addressable thing; a dead pid is
   evicted, a live one is refused.
2. **Presence** — a topic per addressable thing, created on first LIVE connection and greeted, so it
   is visible in the list at all. A lane gets its own, titled `<project> · <lane>` and clipped from
   the left so that lanes of one project are told apart by their tails rather than their heads.
3. **Delivery** — say / ask / done reach the phone, budgeted per chat, clipped on a character
   boundary, audited before and after, acked exactly once.
4. **Resolution** — a tap becomes the option the bridge minted, resolved against a written record,
   answerable once.
5. **Retirement** — buttons come off a question that has closed, whoever closed it.
6. **Alarm** — the watchdog, which shares no code, no process and no runtime with the hub, because
   an alarm that dies with the thing it watches is not one.

## Explicitly NOT capabilities of the hub

Each of these is a line, not an omission:

* **Spawning, supervising, or killing anything.** Not from anything inbound: no message, no tap and
  no frame reaches a process. The binary's only three shipped `Command` call sites are in
  `cmd/enroll.rs`, reached from `argv` at the terminal, each naming `git` as a literal with a fixed
  subcommand — `nothing_inbound_can_start_a_process.rs` goes red if a fourth appears, if a program is
  built from a value, or if the door, the hub, the bot, the surface, the pacer or the registry names
  a way to start anything at all.
* **Knowing what a lane, a proof, a worktree, or a re-ground is.**
* **Choosing a model, an engine, or a repository.**
* **Enrolling a project or minting a secret.** Terminal-only, so a message cannot grant itself access.
* **Reaching a model**, except one call site, agent to operator, machine-checked.
* **Typing into a terminal.** The path was deleted rather than gated; the guard now forbids naming a
  write RPC anywhere.

## What to decide before building

1. ~~**Is a lane its own topic?**~~ **DECIDED, 2 September: yes, its own.** The document suggested a
   tagged voice inside the project's topic; the operator chose a topic per lane because he wants a
   worktree's rolling context in one place. He was told the cost — roughly twelve permanent topics a
   day, and this design deliberately deletes none — and took it: *"if we need topic cleanup we'll do
   it."* So topic retirement is out of scope, and two consequences are written down rather than
   solved:

   * **`projects.json` grows without bound**, and it turns out not to matter: measured, a lane row
     costs 40 bytes, a year of twelve a day is about 175 KB, and a full parse of a 360-lane file
     takes half a millisecond on an admission that happens a dozen times a day. The file that
     actually compounds is `asks.json`, which is rewritten whole on every ask and every tap — and
     that one is now bounded twice over, by the sweep below and by a shelf life.
   * **A lane that never comes back was leaving its open questions' keyboards on the phone.**
     ~~Nothing of its own ever arrives to sweep it.~~ **Closed, 3 September.** The sweep stays scoped
     to the conversation — scoped to the project it took live lanes' questions away — and a second,
     narrower one runs beside it: when any bridge of a project arrives, every open question of that
     project whose address nothing holds AND whose asking process is no longer running comes off the
     phone. Both facts together are what make it safe; the claims map alone is not, because a bridge
     that merely lost its socket is unclaimed for a second and still waiting. Beyond that, a record
     older than Telegram's 48-hour edit window is dropped rather than kept: past it no keyboard can
     be taken off by anybody, so the record can no longer do the one job it is for.
   * **The delivery budget is what a dispatch day actually hits.** A brand-new conversation costs two
     of the chat's eighteen minute-tokens before its agent speaks — the topic and the greeting that
     makes it visible — so about six new conversations a minute can open, across the whole forum.
     The agent is told when its own cannot; the operator is not told anything. Named and left, in
     `docs/MULTIPLEXER-READINESS.md` §3.
2. **Does the launcher exist at all?** Everything except seam ④ works without it, and refusing it is
   coherent: enrolment is terminal-only, so starting could be too.

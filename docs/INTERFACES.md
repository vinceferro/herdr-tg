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
          │  ① hub-proto over a Unix socket        ← BUILT
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

`crates/hub-proto`. NDJSON over `AF_UNIX`, nine frames up, six down. Zero mentions of herdr,
kickoff, claude, tmux or panes in `hub.rs`.

The contract, in one line each:

* **Identity is a secret, never a name.** `hello` carries no display name; the hub resolves the
  secret to a project and takes the title from its own registry.
* **Authority flows one way.** No frame carries addressing. The hub knows which connection is which.
* **Every frame is acked exactly once**, with three delivery values — `yes`, `no`, `unseen` —
  because a send that timed out may or may not have landed and there is no way to ask.
* **A tap resolves against a written record**, never a button's position.
* **Skew is normal**: unknown frame kind is ignored, unknown field is ignored, major version
  mismatch is refused.

**The one widening this seam needs:** `hello` gains an optional `lane`, and the claim key becomes
`(project, lane)`. The secret still proves only the PROJECT — a lane and its project are one repo
and one trust domain, so a bridge naming a lane can only ever affect its own.

## Seam ② — adapter ↔ engine. Engine-specific by definition.

This is where the two engines stop resembling each other, and that is fine: it is the only place
they are allowed to.

| | Claude Code | opencode |
|---|---|---|
| attachment | MCP channel plugin, in-process | HTTP + `/event` stream, out-of-process |
| lifetime | dies with the session | outlives sessions |
| granularity | one session | one server, many sessions |
| status | BUILT, proven against the real hub | not built |

Consequence worth stating plainly: the opencode adapter is **one bridge per project, not per
lane**, holding one hub connection per lane. The Claude adapter is one per session because that is
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
   is visible in the list at all.
3. **Delivery** — say / ask / done reach the phone, budgeted per chat, clipped on a character
   boundary, audited before and after, acked exactly once.
4. **Resolution** — a tap becomes the option the bridge minted, resolved against a written record,
   answerable once.
5. **Retirement** — buttons come off a question that has closed, whoever closed it.
6. **Alarm** — the watchdog, which shares no code, no process and no runtime with the hub, because
   an alarm that dies with the thing it watches is not one.

## Explicitly NOT capabilities of the hub

Each of these is a line, not an omission:

* **Spawning, supervising, or killing anything.** There is no `Command` in the binary.
* **Knowing what a lane, a proof, a worktree, or a re-ground is.**
* **Choosing a model, an engine, or a repository.**
* **Enrolling a project or minting a secret.** Terminal-only, so a message cannot grant itself access.
* **Reaching a model**, except one call site, agent to operator, machine-checked.
* **Typing into a terminal.** The path was deleted rather than gated; the guard now forbids naming a
  write RPC anywhere.

## What to decide before building

1. **Is a lane its own topic?** Lanes are ephemeral, topics are permanent, and there were twelve
   lane worktrees in one day. Suggested: lanes speak in the project's topic, tagged, and get their
   own only when the operator asks for one.
2. **Does the launcher exist at all?** Everything except seam ④ works without it, and refusing it is
   coherent: enrolment is terminal-only, so starting could be too.

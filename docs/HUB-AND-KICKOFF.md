<!-- SKETCH, written 2026-09-01 with the operator, from what was on the box then. Read it as
     history plus three answers. The two questions at the end are ANSWERED — the operator ruled on
     the first on 2 September and it is built; the second is answered by the architecture, not by a
     preference. Section 3's open addition is answered too, by kickoff, on 3 September. Each says so
     where it stands. Everything else here still predates the conversation model
     (docs/CONVERSATIONS.md), the exact-session binding and the transport seam, and it has not been
     revised to match; where this sketch and docs/ATTACHING.md disagree, ATTACHING is the contract. -->

# Wiring the hub to kickoff

## What kickoff is for, in the operator's words

> The problem it solves is having a new context feeling as smart as the one you left behind.

That is the whole thesis, and everything mechanical follows from it. A session is disposable; the
**files** are not. So the discipline is: write what is durable, cycle the session, and re-ground
from the files rather than from a conversation nobody can replay. `KICKOFF.md` says it plainly —
measure headroom, hand off, cycle — and the handoff is what makes the cycle free.

The restart is not a recovery mechanism. It is the normal operation.

## What is actually running, today

Three lifecycles, and they are not equally supervised.

| | started by | outlives | restarted at boot |
|---|---|---|---|
| **the hub** (`herdr-tg serve`) | `systemd --user` | everything | yes, with linger |
| **a supervisor** (`supervisor.sh`) | `kickoff up`, by hand | its sessions | **no** |
| **a coordinator session** | `session-run.sh` | nothing | by its supervisor |
| **a lane** | `lane-dispatch.sh` | nothing | not at all |

There are **zero supervisors running on this box right now**, and no systemd unit that would start
one. That is the first gap and it is the load-bearing one: every "do it from the phone" idea below
assumes something is alive to be asked.

## Two engines, two attachment points

This is the fact that decides the wiring, and it is easy to miss.

* A **coordinator** is a Claude Code session. It attaches to the hub through `kickoff-channel`, an
  MCP channel plugin inside the session. Built, and proven against the real hub over a real socket.
* A **lane** is an **opencode** session, created over opencode's HTTP API
  (`POST /session`, then `prompt_async`) against a worktree, and recorded in `.kickoff/graph.json`
  with its session id, port, branch, worktree and proof command.

So the coordinator's route to the phone exists and the lane's does not. A lane cannot use
`kickoff-channel`: there is no Claude Code process to host it.

## What a lane already gives us for free

`lane-dispatch.sh` is better than anything the hub would invent:

* a git worktree and branch per lane, so parallel lanes never collide in one tree
* a node in a dependency graph, with `deps`
* **completion is machine-derived** — `PROOF_CMD` runs in the lane's worktree after the sentinel;
  pass is done, fail is proof-failed, and no declared proof is *never* done

What it lacks is a person: *"nothing watches the lane automatically"*, *"the operator's chat stays
free"*. A lane that fails its proof at 3am tells `lane-status.sh`, and nothing else.

## The wiring

### 1. One opencode bridge per project, not per lane

opencode is one server with many sessions and a single `/event` stream. So the bridge is one
process per project that:

* reads `.kickoff/graph.json` for the session-id → lane-id mapping
* consumes `/event`, and turns a lane's output into hub frames
* holds one hub connection **per lane**, so each lane is addressable

It attaches to a public, versioned surface — `serve`, `/doc`, `/event`, measured on opencode
1.18.25 — so it needs no plugin and no patching of opencode.

### 2. The hub's identity model widens by one field

Today: one project, one secret, one claim, one topic.

A lane needs its own address, so `hello` gains an optional `lane`, and the claim key becomes
`(project, lane)`. The coordinator is simply `lane: none`.

**The secret still proves the PROJECT, and that is enough.** A lane and its project are the same
repo, the same worktree parent and the same operator — one trust domain. A bridge that names a lane
can only ever affect its own project, which is the property that matters. The registry stays
project-level; lane topics are bound as lanes appear.

### 3. Re-grounding is one path for everyone

The mechanism already exists and kickoff's own header calls it *"the single mechanism the
supervisor watches, and every trigger funnels through it"*:

    touch <repo>/.kickoff/refresh-requested

The agent can already do this itself — it is in its charter. The operator should be able to ask for
it from a topic. **The hub does not restart anything**; it touches the flag, and the supervisor,
which owns the process group, does the killing and restarting.

**Who owns the restart: answered, 3 September, by kickoff.** Restart and re-grounding are theirs
exclusively. The hub carries the operator's request as an input and never owns the killing or the
respawning: it touches the flag, their supervisor does the rest. The partition in one line — we own
conversation routing, they own the birth, death and rebirth of every session on the channel.

One addition worth building: a refresh from the phone should be a two-tap confirm that says what it
is about to throw away — *"this session is working on X. Refresh anyway?"*. The design forbade
refresh-from-chat because *"a refresh costs a worker its in-flight context"*, and that was right
when the operator could not see what was in flight. A topic can show him. That is a thing a
screen-scraper could never have offered.

### 4. Starting things, without the hub gaining a shell

The hub spawns nothing today — nothing a message, a tap or a frame reaches names a `Command`, and
the binary's only three are enrolment's, on `argv` at the terminal — and that is why §8 can say an inbound
message cannot reach a shell. Keep it.

Instead: **the hub starts named systemd units that already exist.**

    systemctl --user start kickoff@<project>

The set of things it can start is exactly the set someone installed a unit for. Inbound content
still selects from what the machine knows and never names, so there is no string from the wire to a
command line.

This needs one templated user unit, `kickoff@.service`, running `supervisor.sh` for a repo. It is
the piece nobody has written, and it closes the first gap as a side effect: after a reboot, every
enabled project has a supervisor again, so every intent file has a watcher.

## The two questions that were the operator's — both answered

### Q1 — is a lane a topic, or a voice in the project's topic? ANSWERED: a topic per lane

The operator ruled on 2 September, and it is built and proven live: two worktrees of one project ran
at the same moment and each got its own forum topic beside the project's. The middle option below —
a lane speaking in the project's topic until he taps for one of its own — was not taken. The
sprawl argument was real and is answered elsewhere: a vacant room is not listed until something has
connected to it, and a conversation is a row rather than a directory (docs/CONVERSATIONS.md).

The reasoning as it stood:

A lane is **ephemeral** (hours) and a forum topic is **forever** — the design refuses to delete
topics, because deletion needs `can_delete_messages` and posting into a *closed* topic is
undocumented. There are twelve lane worktrees from 28 August alone.

* **A topic per lane** is the better reading experience and produces sprawl.
* **Lanes in the project's topic**, each line tagged with its lane, produces no sprawl and
  interleaves several conversations.

A middle option: lanes speak in the project's topic by default, and a lane gets its own topic only
when the operator taps for one. That keeps the common case tidy and makes the sprawl a choice.

### Q2 — does the hub get to ask systemd to start units? ANSWERED: no, and not by preference

The architecture answers this one rather than a taste for caution. The hub is the communication,
identity, presence and decision plane; it is not the process scheduler and must never gain an
arbitrary command-execution surface. What starts a process is a launcher, which is simply another
adapter: it holds a hub connection, offers what it may start, and acts on the operator's choice
itself. So the hub still causes no process to exist, and `/new` from a phone is possible through
that adapter rather than through a shell the hub grew. Lifecycle intentions may travel as typed,
capability-checked operations against specs the launcher has declared in advance — never as an
image, a command, a mount, a secret, an environment variable or a host path from Telegram.

The reasoning as it stood:

It is a real widening. `systemctl --user start kickoff@x` cannot become a shell, but it is still the
hub causing a process to exist, and today it causes none.

Saying no is coherent: enrolment is terminal-only, and starting could be too. Saying yes is what
makes `/new` from a phone possible at all.

## What is NOT in this sketch

* Multi-operator. One allowlist, one human.
* A lane answering a question. Lanes are background work with a machine-checked proof; if a lane
  needs a person it should fail its proof, not grow a keyboard.
* Deleting `bridge-reap.sh` or the per-project tokens. That is only after the hub's own deaf-worker
  alarm is proven against a deliberately broken bridge.

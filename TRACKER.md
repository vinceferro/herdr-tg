# TRACKER — herdr-tg

Single source of truth for what is in flight. The chat is not. Updated by hand: `.kickoff/bin/mc`
is dead in this repo (the pinned core dropped Mission Control from the public line).

Last updated: 2026-09-02.

## Now

- **The round trip has never run with a live Telegram.** Everything up to the Bot API is proven —
  the real bun bridge against the real hub over a real socket, both directions. The last hop needs
  the operator to start a session with:

      claude --channels plugin:kickoff-channel@herdr-tg-local

  Expect a topic named `herdr-tg` in the forum with a greeting, about five seconds after the
  session starts. If nothing appears, `journalctl --user -u herdr-tg -f` says which half failed:
  "did not become live" means the channel was refused; silence means the flag did not take.

## Done

- The watchdog. Fired three times for real; shares no code or process with the hub.
- `crates/hub-proto` — the wire contract. Nine frames up, six down.
- `lock.rs` — one hub per state dir, taken before the token is read.
- `heartbeat.rs` — the hub's half of the watchdog contract.
- `hub.rs` — socket, SO_PEERCRED, secret-resolved identity, one claim per project, tap ledger, audit.
- `registry.rs` + `herdr-tg enroll` — terminal-only admission, SHA-256 only, secret never stored.
- `queue.rs` — per-chat budget. Paces on the one-second gap, sheds only on the per-minute ceiling.
- `surface.rs` — the real Telegram side.
- `plugins/kickoff-channel` — the Claude Code adapter, proven against the real hub.
- Typed relay — the operator types in a topic and it reaches the agent as a message in its own turn.
- **The screen-scraper deleted**: −9,513 lines. No flag, no mode, no path from Telegram to a keyboard.
- Three adversarial review rounds: 19, 7 and 9 distinct defects, all closed.

## Next, in the order it makes sense

1. **The `lane` widening.** `hello` gains an optional `lane`; the claim key becomes
   `(project, lane)`. The secret still proves only the project — a lane and its project are one
   trust domain. Needed before any opencode work. See `docs/INTERFACES.md` seam ①.
2. **The opencode adapter.** One bridge per project, not per lane: opencode is one server with many
   sessions and a single `/event` stream. This is the test of whether the protocol is really
   engine-agnostic.
3. **The launcher**, if the operator wants it — an adapter that offers what it can start as buttons
   it minted itself, so the hub still spawns nothing. Seam ④.

## Decided

- **One product, no modes.** A boolean was choosing between a socket hub and a screen-scraper —
  two different things to allow on a machine, not two configurations. The scraper is gone.
- **The hub's vocabulary is conversation, never orchestration.** It knows about things that can say,
  ask and end. It does not know what a lane, a proof, a worktree or a re-ground is.
- **The plugin lives in this repo**, not kickoff's marketplace: `kickoff-local` points at the pinned
  core, which an engine hop replaces wholesale.
- **Enrolment stays terminal-only.** A message that can enrol can grant itself access.

## Open questions — the operator's

1. **Is a lane its own topic?** Lanes are ephemeral, topics are permanent; twelve lane worktrees
   were created in one day. Suggested: lanes speak in the project's topic, tagged, and get their own
   only when asked for.
2. **Does the launcher exist at all?** Everything except starting a stopped project works without
   it, and refusing it is coherent.
3. **Rename `kickoff-channel`?** It contains no kickoff logic — the name says who launches it. Cheap
   to change now, a fleet migration later.

## Not in this repo

- `MEMORY.md` is still hand-maintained and has already drifted from the frontmatter it duplicates.
  Proposal (mailed to kickoff, not acted on): generate it at boot, stop tracking it. It also
  removes the one file every parallel worktree would have to append to.

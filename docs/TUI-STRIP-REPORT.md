# The TUI work strip — what landed, and what it is waiting on

**Branch:** `lane/tui-work-strip` · **Date:** 2026-10-04 · **Half:** the consumer half of the
omnibus-dispatch spec (§5 item 4) — the strip that renders `wip` frames and carries ONE
dispatch affordance.

This is the record for whoever picks the work up next: what exists, where the seams are, what
is deliberately not built, and the list of things to re-verify when the frame-owning half
lands.

---

## 1. What landed

A new subcommand, `kickoff-channel strip`, and the module behind it
(`crates/kickoff-channel/src/strip/`):

| File | Owns |
|---|---|
| `frames.rs` | The `wip` / `dispatch` / receipt view models — consumer-side, field names byte-matching the spec's §3.2/§3.3. Timestamp parsing, age spelling, frame routing (`t` → wip/ack/unknown-skip). |
| `board.rs` | The board's one law: a snapshot **replaces**; a lane the newest snapshot does not name is gone. Deltas and unmarked frames are refused with a reason the strip prints. |
| `render.rs` | The strip a person reads: one row per lane (agent · lane · state · last beat · proof), measured widths, honesty rules (below). |
| `seam.rs` | The fixture seam: replays an NDJSON of down-frames at the relay's cadence, appends dispatch frames to an outbox, mints `disp-<ulid>` ids. **Cancel-safe by construction.** |
| `picker.rs` | The one affordance: agent + task file + reason → exactly the spec's `spawn` frame. Fail-closed on half-answers. Receipt verdicts → operator sentences. |
| `mod.rs` | The run loop: renders on arrival, ages on a terminal, calls itself stale or ended, correlates receipts, keeps the last few notes under the board. |
| `cmd/strip.rs` | The clap surface (`--replay`, `--outbox`, `--once`, `--cadence-ms`, `--stale-after-ms`, `--dispatch-id`). |

Fixtures live in `crates/kickoff-channel/tests/fixtures/strip/` — synthetic frames,
byte-matching the spec's field names (`walk`, `delta-unknown`, `triage`,
`dispatch-accepted`, `dispatch-rejected`, `silent`).

### The honesty rules the renderer holds

- **`unverified` is a claim of nothing**: `none declared`, never dressed as `passed`. `done`
  without a recorded proof says `none recorded` rather than borrowing `passed`. A failed
  proof says `FAILED`. A running lane's proof says `not yet run`. Unknown statuses render
  verbatim and the proof column claims nothing (`—`).
- **Stale is said, never implied**: silence past the window draws a banner naming it, and the
  rows below are explicitly "the last known, not live". An ended replay says the same about
  itself.
- **Truncation is carried through**: `showing N of M` when the frame's `shown` < `total`.
- **Last beat ages**: recomputed from `updated` so it grows between frames; falls back to the
  frame's `age_min` marked `≈` (it does not age); a dash when neither is on the wire.

### The proposed triage-provenance field (render-only)

The coordinator's 2026-10-04T14:40Z proposal (Jev seam-1 composition) is folded as an
**optional** field: `triage: { source: "state" | "coordinator" | …, confidence }`. Absent
means absent — no line is rendered, and a frame-owning half that never adopts the field sees
zero difference. `state` renders as `answered from state`, `coordinator` as `woke the
coordinator`, and any other value renders as itself (forward-safe, the same law as unknown
statuses). The model and both render sites are marked PROPOSED-pending-contract in comments.

## 2. The fixture seam, and how the live half plugs in

Nothing live emits `wip` frames yet (their half — the lane-relay, the dispatchd, the
`FLEET-WIP-FRAMES.md` contract — was building as lane-1004-150138 while this landed). The
strip therefore reads from a replay:

```
kickoff-channel strip --replay <frames.ndjson> [--outbox <path>] [--once]
                      [--cadence-ms 2000] [--stale-after-ms N] [--dispatch-id ID]
```

- **Down**: one NDJSON file, one frame per line (`t: "wip"` boards, `t: "ack"` receipts),
  replayed with a 2000 ms cadence by default — the relay's own coalescing floor (spec §4 D2),
  so the strip's rhythm here is the rhythm it will have live.
- **Up**: dispatch frames are appended as JSON lines to the outbox. Nothing executes: the
  spec's division is that the frame owner's dispatchd executes, behind their hub and their
  dispatch-capable token (§3.3), and this strip contains no process-starting code at all —
  the repo's `nothing_inbound_can_start_a_process` guard passes for the plainest reason.
- **Replacing the replay**: the loop only knows the seam's two operations (hand down a frame;
  take a dispatch frame). A live seam (dial the hub, subscribe, emit over the wire) implements
  the same two and nothing else changes — board, renderer and picker are seam-agnostic.

While it runs: `d` opens the pick, `q` leaves, everything else typed is ignored — the strip
is a reading surface with ONE verb, on purpose.

## 3. The [confirm] list — re-verify when their half lands

1. **The contract doc.** `hub/FLEET-WIP-FRAMES.md` was landing with their half. When it
   exists: diff its field names and optionality against `strip/frames.rs`, whose decode tests
   anchor to the spec's own example bytes (`the_spec_s_own_example_frame_decodes_field_for_field`).
   Specifics to confirm:
   - **`full` presence**: this strip fails closed — a frame that does not say `full: true` is
     refused. If their relay legitimately omits `full` on snapshots, that refusal is wrong and
     the contract must say which.
   - **`lane` on `wip`**: decoded, not acted on. Confirm the hub stamps it (fleet-relay
     precedent) or drop it.
   - **The status set**: running, claimed, done, proof-failed, unverified, failed, blocked,
     pending (lanes-snapshot's list). Unknown statuses already render verbatim, so drift is
     survivable — but the proof-column derivation should be reconciled to their final list.
   - **Receipt verdicts**: `accepted | rejected | held` with `why ∈ no_worker | over_budget |
     unknown_agent | bad_task_ref | not_permitted | lane_busy`. `held` (§4 D5) renders as its
     own sentence already; unknown codes render verbatim.
2. **The triage field shape.** `{source, confidence}` is this strip's minimal reading of the
   proposal, not a contract. When Jev's seam-1 composition lands as one, reconcile — including
   deleting the field outright if it is refused.
3. **`disp-<ulid>`**: ids are minted as 26-char Crockford-base32 ULIDs. Confirm their
   dispatchd echoes exactly this in the ack's `ref`. (`--dispatch-id` exists so a scripted
   replay can pin the id a fixture receipt names — it is a replay affordance, not a live one.)
4. **The inline task form.** The spec's words are "a path on the box, or `--Text` inline";
   this strip keeps the `--` marker IN `task_ref` (the executor tells the forms apart by
   those bytes). Confirm their executor reads it that way and not as a separate field.
5. **The receipt ring.** A receipt that arrives before the dispatch it answers (only a replay
   can do this; live wire cannot) is held and matched when the command lands, capped at four.
   Confirm no live consumer multiplexes foreign acks down one connection such that the ring
   would misattribute — after the cap, unmatched acks are dropped, which is correct only if
   they were never ours.
6. **Mounting inside the TUI itself.** `herdr` (the TUI the founder sits at) is a separate,
   closed binary — its source is not on this box, so "a herdr component" could not land inside
   it from here. The strip landed in this repo's product as a terminal surface instead. If the
   intent was in-TUI, that mount is theirs to pick up against this seam (or their own).

## 4. Gates

- **Floor → final.** Clean `main` before the work: **948 passed / 0 failed / 15 ignored**
  (`env -u RUSTUP_TOOLCHAIN TMPDIR=/tmp PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace`).
  The known 1-in-3 flake (`an_ambient_proxy_must_not_be_able_to_reroute_the_gist`) did not hit
  either run. Final count is in the report that shipped with the branch; it must be ≥ floor.
- **RED before GREEN (the named sin).** The ghost-row test was written first and run against
  a deliberately-wrong accumulate-only board: it failed exactly as it should —
  `left: ["lane-a", "lane-b"], right: ["lane-b"]` — before the replace-on-snapshot fix went
  in. Evidence is in the lane task output; the shipped test never ran green-first.
- **A second bug the smoke run caught, also RED-first before the fix.** `next_down` parked a
  popped frame in the future's own locals while its cadence wait ran; a `select!` arm that
  loses is a future **dropped**, so a scripted receipt racing the piped keyboard vanished and
  the strip ended claiming an accepted dispatch had no receipt. Fixed by staging partial
  state in the seam itself (the repo's own event-stream law) and pinned by
  `a_frame_waited_on_through_a_cadence_is_not_lost_when_the_wait_is_abandoned`.
- **Headless smoke** (all piped, no terminal, no sockets invented):
  - `kickoff-channel strip --replay …/walk.ndjson --cadence-ms 50` — three boards render, the
    departed lane disappears at wip-42, every proof state is distinct, the end banner shows.
  - `printf 'd\n1\n/abs/LANE-TASK.md\nreason\n' | kickoff-channel strip --replay …/dispatch-accepted.ndjson
    --outbox … --dispatch-id disp-demo-1 --cadence-ms 25` — the outbox holds exactly one spec-shaped
    spawn frame; the receipt renders. Run 8× at 5 ms cadence to chase the race: 8/8.
- **Repo gates**: the pre-commit six (secrets, identity, fmt, clippy `-D warnings`, doc,
  tests) passed on every commit.

## 5. Posture: what this strip will never do

- **Execute a dispatch.** The affordance emits a frame into the seam and renders the receipt.
  The spec's dispatch path (hub → dispatchd → the proven scripts, behind a dispatch-capable
  token) is the only path, and nothing here bypasses it — there is no shelling out, no
  graph.json writes, no second dispatch route. No conflict with the surface-doctrine posture
  was found, because there is no live path to conflict with yet: the affordance is honest
  about being fixture-seam-only, and it goes live exactly when their executor does.
- **Speak for the hub's vocabulary.** The frame types live in this crate as consumer-side
  view models; `hub-proto` remains conversation-only, as `docs/INTERFACES.md` demands.
- **Grow verbs by accretion.** One reading surface, one verb, one seam.

## 6. Open render choices (deliberate, small)

- `respawns` is decoded but not rendered — a lane that respawned is operationally interesting,
  and the natural home is a suffix on the last-beat column when someone asks for it.
- `plan` (spec §7, slice 1.5) is not modelled: unknown fields are ignored by the decoder, so
  a frame carrying one parses today and the strip stays forward-safe until that slice lands.

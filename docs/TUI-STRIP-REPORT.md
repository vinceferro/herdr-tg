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
| `frames.rs` | The `wip` / `dispatch` / receipt view models — consumer-side, field names byte-matching the landed contract (v1 + slice 1.5), decode-anchored to its own example and canonical fixture bytes. Timestamp parsing, age spelling, frame routing (`t` → wip/ack/unknown-skip). |
| `board.rs` | The board's one law: a snapshot **replaces**; a lane the newest snapshot does not name is gone. Deltas and unmarked frames are refused with a reason the strip prints. |
| `render.rs` | The strip a person reads: one row per lane (agent · lane · state · last beat · proof), measured widths, honesty rules (below). |
| `seam.rs` | The fixture seam: replays an NDJSON of down-frames at the relay's cadence, appends dispatch frames to an outbox, mints `disp-<ulid>` ids. **Cancel-safe by construction.** |
| `picker.rs` | The one affordance: agent + task file + reason → exactly the contract's `spawn` frame (`-Text` inline, one dash — the lane-dispatch.sh convention). Fail-closed on half-answers. Receipt verdicts → operator sentences. |
| `mod.rs` | The run loop: renders on arrival, ages on a terminal, calls itself stale or ended, correlates receipts, keeps the last few notes under the board. |
| `cmd/strip.rs` | The clap surface (`--replay`, `--outbox`, `--once`, `--cadence-ms`, `--stale-after-ms`, `--dispatch-id`). |

Fixtures live in `crates/kickoff-channel/tests/fixtures/strip/` — synthetic scenarios
byte-matching the contract (`walk`, `delta-unknown`, `triage`, `dispatch-accepted`,
`dispatch-rejected`, `silent`), plus the contract's canonical frames copied verbatim
(`contract-plan`, `contract-bare`).

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

The frame-owning half HAS landed (their lane-relay + dispatchd + contract, merges `255d124` /
`38e1b07`); the strip still reads from a replay because the live wire (dial the hub, subscribe,
emit) is the bridge half neither side has wired yet. The replay's frames are now of two kinds:
synthetic scenarios byte-matching the contract, and **the contract's own canonical fixture
bytes copied verbatim** (`contract-plan.ndjson`, `contract-bare.ndjson` — normalized real
relay output from their repo, pinned by decode and render tests field for field).

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

## 3. The [confirm] list — re-verified 2026-10-04 against the landed contract

The contract landed: `hub/FLEET-WIP-FRAMES.md` **v1 (2026-10-04) + the slice 1.5 amendment**, in
the frame-owning repo's main (merges `255d124` slice-1 wip+dispatch+lane-relay+dispatchd, and
`38e1b07` slice-1.5 plan-visible WIP). Every item below is dispositioned; the reconciliations
landed as commit `0c61314` on this branch, with the contract's own canonical fixture bytes
copied verbatim into `tests/fixtures/strip/contract-plan.ndjson` /
`contract-bare.ndjson` — normalized real relay output — so future drift breaks a decode test
here instead of passing silently.

1. **The contract doc — CLOSED-verified, one part CLOSED-adjusted.** Field names and
   optionality match `strip/frames.rs` field for field (the decode test now anchors to the
   contract's own example bytes, not the spec's):
   - **`full` presence — CLOSED-verified.** The contract's rule 4: every `wip` frame IS a full
     snapshot (`full:true`, monotonic `seq`, coalesced ≥ 2 s). The strip's fail-closed refusal
     of a frame that does not say `full:true` is the contract's own honesty, read from the
     consumer side — no relay legitimately omits it.
   - **`lane` on `wip` — CLOSED-verified.** Rule 2: the hub stamps it (`"lane":"wip"`, the
     relay's claim); a sender-written `lane` is ignored always. Decoded-not-acted was right.
     The synthetic fixtures' `"lane":"fleet"` was reshaped to the real stamp.
   - **The status set — CLOSED-verified.** `running, claimed, done, proof-failed, unverified,
     failed, blocked, pending` is byte-for-byte `scripts/lanes-snapshot.py`'s vocabulary in
     their repo. The proof-column derivation matches contract rule 6's rendering law
     (proof-failed/unverified rendered DISTINCTLY; `unverified` a claim of nothing).
   - **Receipt verdicts — CLOSED-adjusted.** The contract's v0 verdict set is
     `accepted | rejected` **only**: the spec-era `held` (§4 D5 keep-and-deliver) is explicitly
     NOT built — "a command that cannot be taken is REJECTED, never silently dropped"
     (executor down receipts `rejected no_worker`). The strip's first-class `held` arm was
     removed; an unknown verdict renders verbatim, so a future `held` reads honestly on the
     day that slice lands without a release here. `why` is the spec's six plus the contract
     v0's three — `bad_dispatch`, `duplicate_ref`, `spawn_timeout` — now spelled in words;
     `lane_busy` is RESERVED in v0 (steer/stop slice) and its sentence is kept for that day.
     The receipt model also grew the ack envelope's real fields: `delivered:"yes"`
     (ACK-SHAPE-TRAP, untouched), and `lane_id` on accepted — rendered as
     "watch <lane> on the board — accepted is not finished", the contract's own honesty rule.
2. **The triage field shape — STILL-OPEN, unchanged on purpose.** The contract v1 + slice 1.5
   does NOT carry `triage`: the §7 amendment adopted `plan` + `progress_snapshot` instead, and
   nothing refuses the proposal either. The field stays optional in the strictest sense
   (absent means absent, no line rendered, PROPOSED-pending-contract in the comments) —
   reconcile when the composition proposal lands as contract or is refused outright.
3. **`disp-<ulid>` — CLOSED-verified.** The ack's `ref` echoes the sender's dispatch id (their
   selftest pins `verdict.ref == dispatch.id`), and `duplicate_ref` refusal means the hub keys
   pending commands on the sender's id — so the id's shape is the sender's choice and
   uniqueness is enforced their side. 26-char Crockford ULIDs satisfy it; `--dispatch-id`
   remains a replay affordance.
4. **The inline task form — CLOSED-adjusted, and this one was more than field-name drift
   (flagged, not redesigned).** The spec said `--Text`; the landed contract and
   `lane-dispatch.sh` strip exactly ONE leading dash (`${TASK_ARG#-}`) and write the rest to
   the lane's task file — the convention is **`-Text`**. A `--Text` frame from this strip
   would have landed a stray `-` as the first byte of a lane's task file. The picker now takes
   the single-dash form (marker kept IN `task_ref`, same law), refuses a bare `-`, and the
   prompt says `-Text`.
5. **The receipt ring — CLOSED-verified.** Contract receipt rules: a dispatch frame is acked
   EXACTLY ONCE, the hub's pending table is the only source of verdict acks, the receipt is
   delivered to the command's sender, and late/unknown refs are ignored hub-side. No consumer
   multiplexes foreign acks down one connection — an ack naming a ref this strip never sent
   was never this strip's, so the ring's hold-then-drop-past-cap is contract-correct.
6. **Mounting inside the TUI itself — CLOSED-verified as theirs-by-design.** The contract's
   honest gap 4 names it: "The TUI work strip is theirs (spec §5 item 4): until the monorepo's
   lane lands, the strip renders nowhere; ours is testable standalone." The strip lives here
   as `kickoff-channel strip`, the terminal surface their contract points at; no in-herdr
   mount is demanded, and herdr stays a separate closed binary.

## 4. Gates

- **Floor → final.** Clean `main` before the work: **948 passed / 0 failed / 15 ignored**
  (`env -u RUSTUP_TOOLCHAIN TMPDIR=/tmp PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace`).
  The known 1-in-3 flake (`an_ambient_proxy_must_not_be_able_to_reroute_the_gist`) did not hit
  either run. Final count is in the report that shipped with the branch; it must be ≥ floor.
- **Re-verify pass (2026-10-04, the contract reconciliation).** Floor at clean `8b19355`:
  **984 passed / 0 failed / 15 ignored**. Final after the reconciliation (`0c61314`):
  **987 passed / 0 failed / 15 ignored** — ≥ floor, +3 new tests (the canonical-fixture
  decode tripwire, the bare-lane/plan-board render proofs). The pre-commit six (secrets,
  identity, fmt, clippy `-D warnings`, doc, tests) passed on the commit; pre-push gates
  passed on the push.
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
- `plan` + `progress_snapshot` (slice 1.5, §7) are now **modelled decode-only** — contract
  field names byte-exact, pinned against the canonical fixture bytes — but still not
  rendered. The contract's own law is carried (absent stays absent, `dirty:false` is a real
  reading not a zero-fill, a bare row renders as a plain row); the choice of HOW a person
  reads a dynamic plan on a terminal strip is a product decision for whoever asks for it, and
  the shapes will be waiting, contract-pinned, when they do.

# KICKOFF.local — this repo's coordinator overrides (adopter-owned)

This file is YOURS. `kickoff pull` NEVER regenerates it (it is a seeded-instance, not a seam),
and eject keeps it by default. Put everything specific to THIS repo here; the pulled
`.kickoff/KICKOFF.md` coordinator charter `@import`s it.

## This repo

- **What it is:** a Telegram front door for a herd of coding agents — it relays what an agent says
  and asks to the operator's phone, and can send a reply back into the pane he meant.
- **Domains + their specialists:** wire protocol → `wire-protocol` · the audited keystroke path and
  its guard → `write-safety` · what leaves the machine → `egress` · the Telegram surface →
  `operator-channel`. Screen interpretation is deliberately unowned; see CLAUDE.md for why.
- **The operator:** steers from a phone in short messages. Asks "how we doing" for a pulse and
  expects a straight answer including "nothing changed". Wants the decision, not the survey — two
  options with a recommendation, never a menu. Will ask for it in plainer words if a choice is
  technical, and that is a signal the framing was wrong, not that he needs teaching. He decides
  scope himself and is comfortable declining depth (he chose to keep the write guard a scanner
  rather than rebuild it). He notices when something is dressed up; report cost honestly, including
  work that turned out to be wasted.

## Conventions that override the pulled charter

- **Report in chat, not to the tracker.** `.kickoff/bin/mc` is dead here — the pinned core dropped
  mission-control from the public line. Never claim a tracker write succeeded; check the exit code
  if you call it at all.
- **Adversarial review is not optional on this repo's write path.** Slice 3 shipped without one and
  came back BROKEN with 7 blockers. Every round since has been green on all five gates before a
  sceptic broke it. Dispatch a sceptic that has not seen the fix, with a mandate to break it.
- **Ask a sceptic to grade each finding "reachable by accident" or "contrived".** That one field is
  what lets the operator decide when to stop, and it is what ended the write-guard arms race.
- **Delegate the reading.** This repo's docs are large — the review alone is 69KB. A specialist reads
  it in its own window; the coordinator reads the conclusion.

## Guardrails specific to this repo

- **Never start the service.** It is stopped by decision, after a review found four ways it could
  type the wrong thing into a real terminal. Restarting it is the operator's call, and only his.
- **Never add a call site to `send_text`, `send_keys` or `send_input`.** One audited path exists.
  The guard that enforces it has been walked past six times; do not become the seventh.
- **Never route around a gate.** If `cargo test` is red, it is red. The one exception is the TMPDIR
  trap in CLAUDE.md, which is an environment fault with a documented prefix — fix the prefix, not
  the gate.
- **This remote is public.** `docs/` holds pasted session transcripts. Anything committed here is
  published; scrub paths, chat ids and session ids before they land.
- **The gist may only summarise agent output for the operator.** One call site, agent to operator,
  never the reverse. Pane text has already left this machine once.

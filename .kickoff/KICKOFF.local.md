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

- **Report in chat AND write the tracker.** `.kickoff/bin/mc` works here again; the line saying it
  was dead outlived the ten-hour outage by two weeks, and while it stood nobody wrote the file. The
  founder's surface renders `.kickoff/state/mission-control/mission-state.json` as this project's
  lanes and plate, so a tracker nobody writes is worse than none — it presents Slice 3 to him as
  current work. The chat still carries the decision; the tracker carries what is true now.
- **Answer where the question came from.** A message that arrives as a `<channel …>` block was typed
  on a phone, and the operator is not reading this transcript. Reply to it through the channel's
  `reply` or `ask`, not only in the terminal — printing an answer he cannot see is the same failure
  as not answering. On 2 September he typed "Restarted" from Telegram and the answer went to the
  terminal alone. A message typed at the keyboard is answered at the keyboard; when both are live,
  the channel carries the decision and the terminal carries the detail.
- **Fail closed on your own tools, not just in the code.** CLAUDE.md's "fail closed" and
  write-safety's "a lookup that comes up empty is a FAILURE, not a silent continue" bind you too.
  Three instances in one session, one shape: three tracker writes whose exit codes were never read,
  reported as "tracker updated" when all three had failed; five charters and two plugins an engine
  hop delivered, dismissed as "not mine" from `git status` without one being opened; an exit code
  read through a pipe to `tail`, which returns tail's status and not the command's. So: read the
  status of what you ran before you report it, and read it unpiped; open what a pull or a hop
  delivered before you decide whose it is; never report an action succeeded on the strength of
  having run it.
- **A crew file that arrives is yours to read.** An engine hop, a `kickoff pull` or a plugin install
  drops charters, hooks and skills into this repo. They arrive untracked and they are not yours —
  which is exactly why they need reading, not dismissing. One of them currently orders a tracker
  update this repo cannot perform.
- **Adversarial review is not optional on the write path, on what leaves this machine, or on
  what the operator is told.** Slice 3 shipped without one and
  came back BROKEN with 7 blockers. Every round since has been green on all five gates before a
  sceptic broke it. Dispatch a sceptic that has not seen the fix, with a mandate to break it.
- **Ask a sceptic to grade each finding "reachable by accident" or "contrived".** That one field is
  what lets the operator decide when to stop, and it is what ended the write-guard arms race.
- **Delegate the reading.** This repo's docs are large — the review alone is 69KB. A specialist reads
  it in its own window; the coordinator reads the conclusion.

## Guardrails specific to this repo

- **Never start the service.** It is stopped by decision, after a review found four ways it could
  type the wrong thing into a real terminal. Restarting it is the operator's call, and only his.
- **The Telegram surface is retired; the hub is demoted, not dead (17 September 2026).** The
  operator revoked the bot token himself (~12:38 CEST) and the same hour `herdr-tg.service`,
  `herdr-tg-watchdog.timer` and `kickoff-hub-attach@oc-dogfood.service` were stopped and disabled,
  so a dead credential would not be polled, refused and nagged about forever. This matches his
  ruling the same day (`.kickoff/memory/telegram-is-a-connector-kickoff-will-drive-it.md`): the
  work surface is the PWA and the herdr TUI; the hub stays the conversation plane for the PWA to
  be proven on, with the Telegram connector arriving later as kickoff work. Ordering, his words:
  PWA chat proven first, then any dismissal. His live comms path meanwhile is agent-mail
  (claude-kickoff's `kickoff-hub.service` — a different system, never touched from here). A
  session's channel tools failing to reach the hub are the expected state, not an outage to fix;
  reviving any unit or minting a token is his call, in that order.
- **Never add a call site to `send_text`, `send_keys` or `send_input`.** One audited path exists.
  The guard that enforces it has been walked past six times; do not become the seventh.
- **Never route around a gate.** If `cargo test` is red, it is red. The one exception is the TMPDIR
  trap in CLAUDE.md, which is an environment fault with a documented prefix — fix the prefix, not
  the gate.
- **This remote is public.** `docs/` holds pasted session transcripts. Anything committed here is
  published; scrub paths, chat ids and session ids before they land.
- **The gist may only summarise agent output for the operator.** One call site, agent to operator,
  never the reverse. Pane text has already left this machine once.

---
name: the-opencode-crew-set-is-inert
description: The 02:24 engine hop delivered five opencode crew charters and two plugins into this repo; they are inert today, and the coordinator one contradicts this repo's live overrides by ordering a tracker read that cannot work here
metadata:
  type: project
---

Committed in `3f6bbe6`. `.opencode/agent/` holds coordinator, builder, planner, reviewer and deployer
charters; `.opencode/plugins/` holds `memory-search.js` and `engine-credit.js`. `AGENTS.md` symlinks
to `CLAUDE.md`.

**Inert, for two reasons:** `WORKER_ENGINE` is commented out in `.kickoff/instance.env`, so this repo
runs `claude`; and this repo's own `opencode.json` is a provider stanza that carries neither
`default_agent: coordinator` nor `instructions: ["AGENTS.md"]`, which is what would load those
charters. Two keys away from live, not zero.

**They contradict this repo where it matters, and nobody has reconciled them.**
`.opencode/agent/coordinator.md` orders a `TRACKER.md` read before acting (no such file exists here)
and names `TRACKER.md / mission-state` as the single source of truth, updated after every unit of
work — which is the dead `mc` seam that [[mission-control-left-the-public-core]] exists to warn about.
`deployer.md` opens "you take a green project to a live URL" on a repo whose service is stopped by
decision. `reviewer.md` is a confirmatory reviewer — run the gates, report green or red — which is
exactly the review that [[a-fix-nobody-attacked-is-a-draft]] proves insufficient here.

**How to apply:** do not switch `WORKER_ENGINE` without reconciling these first. `.kickoff/KICKOFF.local.md`
wins under either engine, but a charter that orders a coordinator to update a tracker that cannot be
written will produce exactly the failure this repo has already had once.

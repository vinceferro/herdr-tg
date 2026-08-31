---
name: mission-control-left-the-public-core
description: RESOLVED 2026-08-31 — the mc tracker shim was dead in this repo for about ten hours — the pin hopped to core-v1.0.0-alpha, whose public line deliberately drops mission-control — and the shim's "engine not present" message misdiagnoses it as a missing clone
metadata:
  type: project
---

At 02:15 on 2026-08-31 `core-v1.0.0-alpha` landed on the box; at 02:24 this repo's pin moved to it,
because a session restart is the hop point (`supervisor.sh:415`). Any restart would have done it.

That line has no `mission-control/`, by ratified decision — `scripts/core-manifest.txt:170` and the
HEAD commit both say so. So `.kickoff/bin/mc <anything>` prints **"kickoff engine not present — see
.kickoff/README"** and exits 1, while `scan-secrets` and `scan-structure` exit 0 from the same pin.

The message is a wrong diagnosis: the engine is present, one component is not, and `kickoff pull`
will never fix it.

**How to apply:** do not chase this as a broken install. `.kickoff/state/mission-control/mission-state.json`
still holds everything written up to its last successful write — `updated_at` reads
2026-08-31T00:23:19Z, mtime 02:23 local, minutes before the hop — and is simply no longer writable
through the seam. Report status in chat instead — and never claim a tracker write succeeded without checking its
exit code, which is exactly the mistake that surfaced this. Reported upstream via agent-mail on
2026-08-31.

---

**RESOLVED, same day.** The finding was reported upstream by agent-mail; claude-kickoff confirmed it
was fleet-wide (rc=1 across ten orgs), fixed the shim's message at `f4c878b`, and repinned orgs to a
line that ships the component. This repo's pin moved again to
`~/kickoff-versions/core-v1.0.1-alpha`, which DOES carry `mission-control/mc-update.py`.

Verified by running it, not by reading the changelog: `.kickoff/bin/mc show` exits 0 and prints the
board. `scan-secrets` and `scan-structure` still exit 0.

**So the tracker works here again.** What survives as a durable lesson is not "MC is missing" — it is
the shape: a pin can move under a running session (twice in one day, at 02:24 and again later), and
what a shim reports about its own absence may misdiagnose the cause. Check the exit code, and check
which core you are actually pinned to, before concluding anything about a seam.

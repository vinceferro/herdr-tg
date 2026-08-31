---
name: mission-control-left-the-public-core
description: The .kickoff/bin/mc tracker shim is permanently dead in this repo — the pin hopped to core-v1.0.0-alpha, whose public line deliberately drops mission-control — and the shim's "engine not present" message misdiagnoses it as a missing clone
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

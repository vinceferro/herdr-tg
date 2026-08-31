---
name: a-fix-nobody-attacked-is-a-draft
description: Across five rounds on slice 3, EVERY round was green on all five gates before a sceptic broke it — so a green suite proves the tests you thought of, and adversarial review is the only thing that found the rest
metadata:
  type: feedback
---

Slice 3 shipped without a review pass and came back BROKEN with 7 blockers. Closing it took five
rounds of fix-then-attack. Every single round ended green — fmt, clippy, tests, doc, secret scan —
and every single round a fresh sceptic found something real, twice a blocker introduced by that
round's own fix.

**Why:** the builder and the tests share an imagination. A regression test written by the person who
wrote the fix encodes the failure they already understand. It cannot encode the one they do not.

**How to apply:**
- Never report a change as done on gate colour alone. Gates are necessary, not sufficient.
- Sceptics get their own window, the diff, and a mandate to break it — not to check it.
- Ask a sceptic to mark each finding *reachable by accident* or *contrived*. That single field is what
  lets the operator decide when to stop, and it ended the write-guard arms race in one question.
- RED before GREEN, always: a regression test that never failed proves nothing. Several rounds here
  produced tests that were green before the fix, and the sceptic caught it by checking out the parent
  commit and re-running them.

---
name: write-guard-stays-a-scanner
description: The operator decided on 2026-08-30 that the D3 write guard stays a source scanner rather than becoming a compiler-enforced private API — fix evasions an honest change could hit, write contrived ones down
metadata:
  type: feedback
---

After three sceptic rounds each found two fresh ways past the guard, the operator was offered two
options: keep the scanner, or make `send_text`/`send_keys` private to herdr-client behind one audited
entry point so the compiler enforces it.

**He chose the scanner.**

**Why:** the threat model is an agent adding a second, unaudited typing path while fixing something
else — not a person deliberately hiding one. Contrived evasions cost nothing real.

**How to apply:** when a new evasion appears, first ask whether an ordinary commit could produce it.
If yes, fix it. If no, record it in the guard's header and move on — do not spend another round on the
arms race without asking him again. The lock-the-door option was declined, not lost: if a fourth or
fifth *accident-reachable* evasion turns up, that is the signal to re-open it.

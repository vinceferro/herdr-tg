---
name: a-unit-name-is-a-shared-namespace
description: We named a new unit kickoff-hub.service, which is another org's live service on this box — the runbook told the operator to overwrite it and our own unit carried a Conflicts= that would have stopped it
metadata:
  type: project
---

On 19 September the app-plane hub was given `deploy/kickoff-hub.service`. That name was already
taken: `kickoff-hub.service` is **claude-kickoff's** NDJSON wire core, enabled and running on this
box (PID 813, up over a day). Two destructive things were in reach at once, and both were written
down as instructions:

- `docs/RUNNING-THE-HUB.md` said `install -m 0644 deploy/kickoff-hub.service ~/.config/systemd/user/…`
  — overwriting their unit file.
- `deploy/herdr-tg.service` had gained `Conflicts=kickoff-hub.service`, so starting OUR phone hub
  would have had systemd **stop their running hub**.

Renamed to `herdr-tg-app.service`. Nothing was committed or installed, so nothing happened; their
unit never moved. A sceptic found it by running `systemctl --user status` on the real box — reading
the diff could not have.

**Why:** a user manager's unit names are ONE namespace shared by every organisation on this
machine, and several live here. Our other units are all prefixed `herdr-tg`, so the convention
existed and this one simply left it. The word "kickoff" feels like ours because the product is
called kickoff-channel — which is exactly the trap, because it feels like theirs to them too.

**How to apply:** prefix every unit, timer and socket this repo ships with `herdr-tg`. Before
adding one, run `systemctl --user list-unit-files '<name>*'` and `ls ~/.config/systemd/user/` —
a name that already exists is not yours. The same goes for any `Conflicts=`, `Requires=` or
`Before=` naming a unit this repo does not ship. Related:
[[a-guard-that-fires-only-on-the-whole-leak]].

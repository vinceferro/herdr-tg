# TRACKER — where the truth about this repo is kept

**This file is a pointer, not a status board.**

It used to be a second copy of the project's state, updated by hand. It drifted, which is what a
second copy does. It was written on the morning of 2 September 2026 and never touched again: the
live round trip to a phone ran that same evening (`docs: the round trip has run`), two of the three
things on its "next" list shipped within hours of it, and six days later the file was still telling
a reader that none of it had happened. A status nobody can falsify is worse than no status, because
it is read as if it were true.

So it stops claiming anything it does not own. Every fact below lives in exactly one place, and this
file says which.

## What is the source of truth for what

| question | the answer lives in | how current it is kept |
| --- | --- | --- |
| what the repo is and where it stands today | `CLAUDE.md`, "The state of the repo" | rewritten by the slice that changes it, dated claim by dated claim |
| what the hub offers, requires and refuses another org | `docs/CAPABILITIES.md` | versioned in its own header; a version bump says what moved |
| how anything attaches to the hub | `docs/ATTACHING.md` | versioned in its own header |
| the four seams, and the closed list of what this project does | `docs/INTERFACES.md` | edited when a seam moves |
| how a project, a room and a lane each get a conversation | `docs/CONVERSATIONS.md` | edited when the identity ladder moves |
| what Telegram actually charges, and what is still unmeasured | `docs/RATE-PROBE.md` | one section per probe, with the run pasted in |
| the product, its limits and its scale envelope, for a stranger | `README.md` | held to the docs above |
| what changed, and when, and why | `git log` | by construction |
| what is in flight right now | the working tree and the branch you are on | by construction |

## Still open

Design questions still open are **OPEN 1–6 in `docs/CAPABILITIES.md`**, with our proposed shape
written beside each. Some of those are the operator's alone to settle; others (2, 3 and 5) are
questions two orgs answer together, which is why that list is headed "joint design, neither side
should harden yet". They are not restated here.

One open question belongs to nobody else's file, so it is kept here:

* **Rename `kickoff-channel`?** The plugin, the marketplace and the namespace all carry the name it
  was given on the first afternoon. Changing it is cheap now and a fleet migration later, and
  nothing else in the repository is tracking that it is still open.

## The rule this file now keeps

1. **Do not write a status here that another file already owns.** If it belongs in `CLAUDE.md` or a
   document under `docs/`, put it there and let this table point at it.
2. **A claim carries its date and its evidence** — a commit, a file and line, or a run that was
   recorded. A sentence nobody can check is not a status.
3. **"Done" is `git log`.** It is the one list that cannot fall behind.

## Not in this repo, and deliberately

* **Mission Control.** The shim `.kickoff/bin/mc` is dead here: the pinned public core does not ship
  the component, so tracker updates are made in files like this one and reported in chat.
* **Anything that names the operator's own machine** — paths, chat ids, user ids, tokens. The hub
  writes its state outside every repository, and the identity gate refuses those strings on any
  added line.

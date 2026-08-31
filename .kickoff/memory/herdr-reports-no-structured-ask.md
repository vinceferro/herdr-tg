---
name: herdr-reports-no-structured-ask
description: herdr's protocol carries agent status and raw screen bytes and nothing about what an agent is ASKING — so any code that answers a prompt is reconstructing structure that was already thrown away, which is why five rounds could not make the write path safe
metadata:
  type: project
---

`AgentStatus` is `Idle / Working / Blocked / Done` (`crates/herdr-client/src/proto/model.rs:49`) plus
`pane.read` bytes. There is no method that says *this agent is asking X, the options are A, B, C*.

So `permission.rs` was never parsing a dialog. It was reconstructing one from a picture of a dialog.
Five adversarial rounds each closed a real defect and each was broken again by an ordinary rendering
nobody had pictured. Against real `tmux capture-pane` output the parser still erred toward typing into
live controls by about six to one.

**Why this is durable and not just history:** it is a property of the protocol, not of our code. Any
future attempt to answer a pane from a rendered screen inherits it. The only real fixes are upstream —
herdr gaining a structured ask, or a per-harness bridge that reads the ask before it becomes pixels.
That is what `docs/HUB-DESIGN.md` proposes.

**How to apply:** if anyone proposes improving the parser, this is the answer. The question is
ill-posed, and effort belongs upstream. See [[write-guard-stays-a-scanner]] for the related decision.

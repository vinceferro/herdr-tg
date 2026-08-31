---
name: the-write-guard-was-walked-past-six-times
description: crates/herdr-client/tests/no_live_write_call_site.rs — the guard that stops an unaudited keystroke path — was defeated six distinct ways across five rounds, always by the same shape: a lookup that came up empty and quietly carried on
metadata:
  type: project
---

The six, in the order they were found: a source directory named `target` (the skip matched bare name
at any depth); `include!` of a non-`.rs` file (both "independent" oracles filtered on `.rs`, exactly
like the walk they were meant to check); a trailing comment on a test module's closing brace; a
`#[rustfmt::skip]` block hand-indented by two spaces; `#[path]` resolved against the wrong base
directory; and a Cargo target path pointing outside every declared member.

All six are closed. The standing property that came out of it is written into the file:

> a lookup that comes up empty is a FAILURE, not a silent continue

and the guard now proves it scanned what ships by cross-checking against `git ls-files`, rather than
asserting it.

**How to apply:** when this guard is touched, ask what assumption the new check inherits from the
thing it is checking. An oracle that shares its subject's blind spot is not an oracle — that was
finding number two, and it is the one that generalises. The two evasions left open need deliberate
contrivance and are recorded as accepted in the file's own header. See
[[write-guard-stays-a-scanner]].

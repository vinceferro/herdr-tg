# A proof staged in /tmp proves nothing under PrivateTmp

`PrivateTmp=yes` gives a systemd unit its own empty `/tmp`. Any test that stages state there and
then runs the unit against it hands the unit an EMPTY directory — and the code under test does the
correct thing with an empty directory, exits 0, and the harness reads that as a pass.

Found 2026-08-31 proving the watchdog. `mktemp -d` staged a dead hub; the unit saw nothing to
watch, exited 0 silently, and the run looked identical to success. The give-away was a side effect
that was missing (no latch file), not the exit code.

**Stage under `$HOME` when the unit sets `PrivateTmp=yes`**, and assert on a side effect the code
must produce, never on the exit code alone. `scripts/install-watchdog.sh` does both and records why.

The same shape as the six write-guard bypasses: a lookup came up empty and everything carried on.
See [[the-write-guard-was-walked-past-six-times]].

Second trap from the same session, already in CLAUDE.md for cargo but it bites shell too: `TMPDIR`
arrives here as the literal string `%h/.cache/tmp`, so `mktemp` returns a RELATIVE path. See
[[cargo-needs-a-real-tmpdir]].

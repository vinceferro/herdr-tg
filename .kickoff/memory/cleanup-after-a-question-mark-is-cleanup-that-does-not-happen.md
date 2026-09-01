# Cleanup written after a `?` is cleanup that does not happen on the common path

A `?` in a loop that ends a connection skips everything after it. If the release, the unlock or the
deregistration lives below that line, it runs only when the tidy path is taken — and the tidy path
is usually the rare one.

Found 2026-09-01 in `hub.rs`. The hub released a project's claim after its read loop, behind a `?`.
A bridge that closes while bytes are still unread in its receive buffer makes the kernel send an
RST, so the hub's next read fails with a connection reset rather than ending at end-of-file. A
bridge that is not reading its acks does exactly that, and acks are backpressure, so a busy bridge
reads them late or never. The result: a project holding a claim nobody was behind, its worker
refused as already-connected, gone quiet with nothing anywhere saying why.

**A clean end-of-file is the exception on a socket, not the rule.** Write the teardown so every way
out reaches it — a `loop { match … }` with explicit `break` arms, or a guard object — and pin it
with a test that closes WITHOUT draining, because a test that drains politely never sees this.

`a_bridge_that_dies_without_reading_its_acks_still_releases_its_project` is that test; it was proved
RED against the old shape first. Same family as
[[the-write-guard-was-walked-past-six-times]]: the failure path carried on quietly.

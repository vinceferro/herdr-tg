# The agent scratchpad lives INSIDE this repo, and it breaks the write guard

`TMPDIR` here is the literal string `%h/.cache/tmp`, so it resolves relative to the working
directory — which means the scratchpad is `<repo>/%h/.cache/tmp/...`, inside the workspace root.

Two consequences, both real, both hit on 2026-09-01:

1. **A subagent that copies the repo into the scratchpad breaks
   `no_live_write_call_site.rs`.** The guard walks the workspace ROOT by design (deny by default),
   so a copy of `deliver.rs` under `%h/` is a call site as far as it is concerned. Six sceptics
   from one review left six copies; the suite went red with 30 "violations" in files nobody wrote.
   The guard is right — the fix is not to leave repo copies inside the repo.
2. **Disk.** Those six copies were 8.8 GB. `%h/` is gitignored, so nothing notices.

When a review or a probe needs a copy of the tree, put it under `$HOME`, not in the scratchpad.
After a fan-out that ran cargo, check with:

    find '<repo>/%h' -type d -name crates -prune

and delete what it finds. See [[cargo-needs-a-real-tmpdir]] for where the `%h` comes from.
